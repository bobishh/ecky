mod build123d_lowering;
mod edge_ops;
mod eval_scalar;
mod mesh_ops;
mod model;
mod runtime;
mod shared;
mod sketch;
mod syntax;

use crate::contracts::{AppResult, DesignParams, ParsedParamsResult};
use crate::ecky_scheme::try_compile_to_core_program;
use crate::models::ArtifactBundle;
use crate::PathResolver;

pub fn lower_to_build123d(source: &str) -> AppResult<String> {
    if let Some(program) = try_compile_to_core_program(source) {
        let model = model::core_program_to_model(&program?)?;
        return build123d_lowering::lower_model_to_build123d(&model);
    }
    build123d_lowering::lower_to_build123d(source)
}

pub fn derive_controls(source: &str) -> AppResult<ParsedParamsResult> {
    if let Some(program) = try_compile_to_core_program(source) {
        let model = model::core_program_to_model(&program?)?;
        return runtime::derive_controls_from_model(&model);
    }
    runtime::derive_controls(source)
}

pub fn render_model(
    source: &str,
    parameters: &DesignParams,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    if let Some(program) = try_compile_to_core_program(source) {
        let model = model::core_program_to_model(&program?)?;
        return runtime::render_model_from_model(&model, source, parameters, app);
    }
    runtime::render_model(source, parameters, app)
}

#[cfg(test)]
use std::path::{Path, PathBuf};

#[cfg(test)]
use crate::models::{EngineKind, ModelManifest, ParamValue};

#[cfg(test)]
use self::runtime::{mesh_area, mesh_volume};

#[cfg(test)]
use self::shared::IrMesh;

#[cfg(test)]
include!("tests.rs");
