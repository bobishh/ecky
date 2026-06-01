pub mod backend_capabilities;
mod build123d_lowering;
pub(crate) mod edge_ops;
mod eval_scalar;
mod freecad_lowering;
mod mesh_ops;
mod model;
mod runtime;
mod shared;
mod sketch;
mod syntax;

use crate::contracts::{AppResult, DesignParams, ParsedParamsResult};
use crate::ecky_core_ir::{CoreNode, CoreNodeKind, CoreOperation, CoreProgram};
use crate::ecky_scheme::try_compile_to_core_program;
use crate::models::ArtifactBundle;
use crate::models::ModelManifest;
use crate::PathResolver;

pub fn source_uses_ecky_rust_only_cad_ops(source: &str) -> bool {
    if let Some(program) = try_compile_to_core_program(source) {
        return program
            .map(|program| core_program_uses_ecky_rust_only_cad_ops(&program))
            .unwrap_or(false);
    }

    syntax::ir_parse(source)
        .map(|value| value_uses_ecky_rust_only_cad_ops(&value))
        .unwrap_or(false)
}

pub fn source_uses_exact_backend_only_cad_ops(source: &str) -> bool {
    if let Some(program) = try_compile_to_core_program(source) {
        return program
            .map(|program| core_program_uses_exact_backend_only_cad_ops(&program))
            .unwrap_or(false);
    }

    syntax::ir_parse(source)
        .map(|value| value_uses_exact_backend_only_cad_ops(&value))
        .unwrap_or(false)
}

pub fn source_uses_direct_occt_required_cad_ops(source: &str) -> bool {
    if let Some(program) = try_compile_to_core_program(source) {
        return program
            .map(|program| core_program_uses_direct_occt_required_cad_ops(&program))
            .unwrap_or(false);
    }

    syntax::ir_parse(source)
        .map(|value| value_uses_direct_occt_required_cad_ops(&value))
        .unwrap_or(false)
}

pub fn lower_to_build123d(source: &str) -> AppResult<String> {
    lower_to_build123d_with_previous_manifest(source, None)
}

pub fn lower_to_build123d_with_previous_manifest(
    source: &str,
    previous_manifest: Option<&ModelManifest>,
) -> AppResult<String> {
    if let Some(program) = try_compile_to_core_program(source) {
        let program = crate::topology_target_ids::rebind_program_tagged_selectors(
            &program?,
            previous_manifest,
        )?;
        return build123d_lowering::lower_core_program_to_build123d(&program);
    }
    build123d_lowering::lower_to_build123d(source)
}

pub fn lower_to_freecad(source: &str) -> AppResult<String> {
    lower_to_freecad_with_previous_manifest(source, None)
}

pub fn lower_to_freecad_with_previous_manifest(
    source: &str,
    previous_manifest: Option<&ModelManifest>,
) -> AppResult<String> {
    if let Some(program) = try_compile_to_core_program(source) {
        let program = crate::topology_target_ids::rebind_program_tagged_selectors(
            &program?,
            previous_manifest,
        )?;
        return freecad_lowering::lower_core_program_to_freecad(&program);
    }
    freecad_lowering::lower_to_freecad(source)
}

pub fn derive_controls(source: &str) -> AppResult<ParsedParamsResult> {
    if let Some(program) = try_compile_to_core_program(source) {
        return runtime::derive_controls_from_core_program(&program?);
    }
    runtime::derive_controls(source)
}

pub fn render_model(
    source: &str,
    parameters: &DesignParams,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    render_model_with_previous_manifest(source, parameters, None, app)
}

pub fn render_model_with_previous_manifest(
    source: &str,
    parameters: &DesignParams,
    previous_manifest: Option<&ModelManifest>,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    if let Some(program) = try_compile_to_core_program(source) {
        let program = crate::topology_target_ids::rebind_program_tagged_selectors(
            &program?,
            previous_manifest,
        )?;
        return runtime::render_core_program(&program, source, parameters, app);
    }
    runtime::render_model(source, parameters, app)
}

pub(crate) fn build_core_program_param_env_for_eval(
    program: &CoreProgram,
    parameters: &DesignParams,
) -> AppResult<std::collections::BTreeMap<String, crate::models::ParamValue>> {
    runtime::build_core_program_param_env_for_eval(program, parameters)
}

pub(crate) fn eval_core_number_with_locals(
    node: &CoreNode,
    param_names: &std::collections::BTreeMap<u64, String>,
    env: &std::collections::BTreeMap<String, crate::models::ParamValue>,
) -> AppResult<f64> {
    runtime::eval_core_number_with_locals(node, param_names, env)
}

pub(crate) fn eval_core_bool_with_locals(
    node: &CoreNode,
    param_names: &std::collections::BTreeMap<u64, String>,
    env: &std::collections::BTreeMap<String, crate::models::ParamValue>,
) -> AppResult<bool> {
    runtime::eval_core_bool_with_locals(node, param_names, env)
}

pub(crate) fn eval_core_stringish_with_locals(
    node: &CoreNode,
    param_names: &std::collections::BTreeMap<u64, String>,
    env: &std::collections::BTreeMap<String, crate::models::ParamValue>,
) -> AppResult<String> {
    runtime::eval_core_stringish_with_locals(node, param_names, env)
}

fn core_program_uses_ecky_rust_only_cad_ops(program: &CoreProgram) -> bool {
    program
        .parts
        .iter()
        .any(|part| core_node_uses_ecky_rust_only_cad_ops(&part.root))
}

fn core_program_uses_exact_backend_only_cad_ops(program: &CoreProgram) -> bool {
    program
        .parts
        .iter()
        .any(|part| core_node_uses_exact_backend_only_cad_ops(&part.root))
}

fn core_program_uses_direct_occt_required_cad_ops(program: &CoreProgram) -> bool {
    program
        .parts
        .iter()
        .any(|part| core_node_uses_direct_occt_required_cad_ops(&part.root))
}

fn core_node_uses_ecky_rust_only_cad_ops(node: &CoreNode) -> bool {
    match &node.kind {
        CoreNodeKind::Literal(_) | CoreNodeKind::Reference(_) | CoreNodeKind::Range { .. } => false,
        CoreNodeKind::Build { bindings, result } => {
            bindings
                .iter()
                .any(|binding| core_node_uses_ecky_rust_only_cad_ops(&binding.value))
                || core_node_uses_ecky_rust_only_cad_ops(result)
        }
        CoreNodeKind::Let { bindings, body } => {
            bindings
                .iter()
                .any(|binding| core_node_uses_ecky_rust_only_cad_ops(&binding.value))
                || core_node_uses_ecky_rust_only_cad_ops(body)
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            core_node_uses_ecky_rust_only_cad_ops(condition)
                || core_node_uses_ecky_rust_only_cad_ops(then_branch)
                || core_node_uses_ecky_rust_only_cad_ops(else_branch)
        }
        CoreNodeKind::Call { op, args, keywords } => {
            operation_uses_ecky_rust_only_cad_ops(op)
                || args.iter().any(core_node_uses_ecky_rust_only_cad_ops)
                || keywords
                    .iter()
                    .any(|keyword| core_node_uses_ecky_rust_only_cad_ops(keyword.source_node()))
        }
        CoreNodeKind::Map { sources, body, .. } => {
            sources.iter().any(core_node_uses_ecky_rust_only_cad_ops)
                || core_node_uses_ecky_rust_only_cad_ops(body)
        }
        CoreNodeKind::Apply { op, args, list } => {
            operation_uses_ecky_rust_only_cad_ops(op)
                || args.iter().any(core_node_uses_ecky_rust_only_cad_ops)
                || core_node_uses_ecky_rust_only_cad_ops(list)
        }
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
            items.iter().any(core_node_uses_ecky_rust_only_cad_ops)
        }
    }
}

fn core_node_uses_direct_occt_required_cad_ops(node: &CoreNode) -> bool {
    match &node.kind {
        CoreNodeKind::Call { op, args, keywords } => {
            is_direct_occt_required_core_op(op)
                || args.iter().any(core_node_uses_direct_occt_required_cad_ops)
                || keywords.iter().any(|keyword| match &keyword.value {
                    crate::ecky_core_ir::CoreKeywordValue::Expr(value) => {
                        core_node_uses_direct_occt_required_cad_ops(value)
                    }
                    crate::ecky_core_ir::CoreKeywordValue::Selector { source, .. } => {
                        core_node_uses_direct_occt_required_cad_ops(source)
                    }
                })
        }
        CoreNodeKind::Build { bindings, result } => {
            bindings
                .iter()
                .any(|binding| core_node_uses_direct_occt_required_cad_ops(&binding.value))
                || core_node_uses_direct_occt_required_cad_ops(result)
        }
        CoreNodeKind::Let { bindings, body } => {
            bindings
                .iter()
                .any(|binding| core_node_uses_direct_occt_required_cad_ops(&binding.value))
                || core_node_uses_direct_occt_required_cad_ops(body)
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            core_node_uses_direct_occt_required_cad_ops(condition)
                || core_node_uses_direct_occt_required_cad_ops(then_branch)
                || core_node_uses_direct_occt_required_cad_ops(else_branch)
        }
        CoreNodeKind::List(items) => items
            .iter()
            .any(core_node_uses_direct_occt_required_cad_ops),
        CoreNodeKind::Range { start, end } => {
            core_node_uses_direct_occt_required_cad_ops(start)
                || core_node_uses_direct_occt_required_cad_ops(end)
        }
        CoreNodeKind::Map { sources, body, .. } => {
            sources
                .iter()
                .any(core_node_uses_direct_occt_required_cad_ops)
                || core_node_uses_direct_occt_required_cad_ops(body)
        }
        CoreNodeKind::Apply { args, list, .. } => {
            args.iter().any(core_node_uses_direct_occt_required_cad_ops)
                || core_node_uses_direct_occt_required_cad_ops(list)
        }
        CoreNodeKind::Group(items) => items
            .iter()
            .any(core_node_uses_direct_occt_required_cad_ops),
        CoreNodeKind::Reference(_) | CoreNodeKind::Literal(_) => false,
    }
}

fn core_node_uses_exact_backend_only_cad_ops(node: &CoreNode) -> bool {
    match &node.kind {
        CoreNodeKind::Literal(_) | CoreNodeKind::Reference(_) | CoreNodeKind::Range { .. } => false,
        CoreNodeKind::Build { bindings, result } => {
            bindings
                .iter()
                .any(|binding| core_node_uses_exact_backend_only_cad_ops(&binding.value))
                || core_node_uses_exact_backend_only_cad_ops(result)
        }
        CoreNodeKind::Let { bindings, body } => {
            bindings
                .iter()
                .any(|binding| core_node_uses_exact_backend_only_cad_ops(&binding.value))
                || core_node_uses_exact_backend_only_cad_ops(body)
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            core_node_uses_exact_backend_only_cad_ops(condition)
                || core_node_uses_exact_backend_only_cad_ops(then_branch)
                || core_node_uses_exact_backend_only_cad_ops(else_branch)
        }
        CoreNodeKind::Call { op, args, keywords } => {
            operation_uses_exact_backend_only_cad_ops(op)
                || args.iter().any(core_node_uses_exact_backend_only_cad_ops)
                || keywords
                    .iter()
                    .any(|keyword| core_node_uses_exact_backend_only_cad_ops(keyword.source_node()))
        }
        CoreNodeKind::Map { sources, body, .. } => {
            sources
                .iter()
                .any(core_node_uses_exact_backend_only_cad_ops)
                || core_node_uses_exact_backend_only_cad_ops(body)
        }
        CoreNodeKind::Apply { op, args, list } => {
            operation_uses_exact_backend_only_cad_ops(op)
                || args.iter().any(core_node_uses_exact_backend_only_cad_ops)
                || core_node_uses_exact_backend_only_cad_ops(list)
        }
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
            items.iter().any(core_node_uses_exact_backend_only_cad_ops)
        }
    }
}

fn operation_uses_ecky_rust_only_cad_ops(op: &CoreOperation) -> bool {
    match op {
        CoreOperation::Custom(name) => is_ecky_rust_only_cad_head(name),
        _ => false,
    }
}

fn operation_uses_exact_backend_only_cad_ops(op: &CoreOperation) -> bool {
    match op {
        CoreOperation::Custom(name) => is_exact_backend_only_cad_head(name),
        _ => false,
    }
}

fn value_uses_ecky_rust_only_cad_ops(value: &lexpr::Value) -> bool {
    let Some(items) = value.to_vec() else {
        return false;
    };

    if items
        .first()
        .and_then(lexpr::Value::as_symbol)
        .is_some_and(is_ecky_rust_only_cad_head)
    {
        return true;
    }

    items.iter().any(value_uses_ecky_rust_only_cad_ops)
}

fn value_uses_exact_backend_only_cad_ops(value: &lexpr::Value) -> bool {
    let Some(items) = value.to_vec() else {
        return false;
    };

    if items
        .first()
        .and_then(lexpr::Value::as_symbol)
        .is_some_and(is_exact_backend_only_cad_head)
    {
        return true;
    }

    items.iter().any(value_uses_exact_backend_only_cad_ops)
}

fn value_uses_direct_occt_required_cad_ops(value: &lexpr::Value) -> bool {
    let Some(items) = value.to_vec() else {
        return false;
    };

    if items
        .first()
        .and_then(lexpr::Value::as_symbol)
        .is_some_and(is_direct_occt_required_cad_head)
    {
        return true;
    }

    items.iter().any(value_uses_direct_occt_required_cad_ops)
}

fn is_ecky_rust_only_cad_head(head: &str) -> bool {
    crate::ecky_language_surface::ECKY_RUST_ONLY_CAD_OPS.contains(&head) || head == "pattern"
}

fn is_exact_backend_only_cad_head(head: &str) -> bool {
    crate::ecky_language_surface::EXACT_BACKEND_ONLY_CAD_OPS.contains(&head)
}

fn is_direct_occt_required_cad_head(head: &str) -> bool {
    matches!(head, "text" | "svg" | "import-stl" | "helical-ridge")
}

fn is_direct_occt_required_core_op(op: &CoreOperation) -> bool {
    matches!(
        op,
        CoreOperation::Primitive(crate::ecky_core_ir::CorePrimitive::Text)
            | CoreOperation::Primitive(crate::ecky_core_ir::CorePrimitive::Svg)
            | CoreOperation::Primitive(crate::ecky_core_ir::CorePrimitive::Stl)
    ) || matches!(op, CoreOperation::Custom(name) if name == "helical-ridge")
}

#[cfg(test)]
use std::path::{Path, PathBuf};

#[cfg(test)]
use crate::models::{EngineKind, ParamValue};

#[cfg(test)]
use self::runtime::{mesh_area, mesh_volume};

#[cfg(test)]
use self::shared::IrMesh;

#[cfg(test)]
include!("tests.rs");
