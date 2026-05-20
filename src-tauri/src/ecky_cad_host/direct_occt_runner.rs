use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::direct_occt::{OcctArg, OcctCommand, OcctKeyword, OcctOp, OcctPlan};
use crate::models::{AppError, AppResult, PathResolver};

const PLAN_FILE_NAME: &str = "plan.json";
const RUNNER_RESOURCE_PATH: &str = "runtime/occt/bin/direct-occt-runner";
const LEGACY_RUNNER_RESOURCE_PATH: &str = "bin/direct-occt-runner";
const RUNNER_DISABLED_ENV: &str = "ECKY_DIRECT_OCCT_RUNNER_DISABLED";

/// Tests that need a machine-independent "no runner anywhere" environment
/// disable the CWD-relative fallback paths through a thread-local guard
/// instead of chdir, which would poison every concurrently running test.
#[cfg(test)]
pub(crate) mod test_discovery {
    use std::cell::Cell;

    thread_local! {
        static DISABLE_CWD_FALLBACKS: Cell<bool> = const { Cell::new(false) };
    }

    pub(crate) fn cwd_fallbacks_disabled() -> bool {
        DISABLE_CWD_FALLBACKS.with(Cell::get)
    }

    pub(crate) struct CwdFallbackGuard;

    impl CwdFallbackGuard {
        pub(crate) fn disable() -> Self {
            DISABLE_CWD_FALLBACKS.with(|flag| flag.set(true));
            Self
        }
    }

    impl Drop for CwdFallbackGuard {
        fn drop(&mut self) {
            DISABLE_CWD_FALLBACKS.with(|flag| flag.set(false));
        }
    }
}
const MODEL_STEP_FILE_NAME: &str = "model.step";
const PREVIEW_STL_FILE_NAME: &str = "preview.stl";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunnerPlan {
    schema_version: u32,
    plan_id: String,
    parts: Vec<RunnerPart>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunnerPart {
    key: String,
    label: String,
    root: u64,
    commands: Vec<RunnerCommand>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunnerCommand {
    output: u64,
    op: String,
    args: Vec<RunnerArg>,
    keywords: Vec<RunnerKeyword>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunnerArg {
    kind: String,
    value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunnerKeyword {
    name: String,
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<RunnerArg>,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<serde_json::Value>,
}

pub(crate) fn run_plan_step_stl_if_available(
    plan: &OcctPlan,
    output_dir: impl AsRef<Path>,
    app: &dyn PathResolver,
) -> AppResult<Option<super::direct_occt_sdk::NativeExportOutcome>> {
    run_plan_step_stl_with_mode(plan, output_dir, app, runner_enabled())
}

pub(crate) fn run_plan_step_stl_with_mode(
    plan: &OcctPlan,
    output_dir: impl AsRef<Path>,
    app: &dyn PathResolver,
    enabled: bool,
) -> AppResult<Option<super::direct_occt_sdk::NativeExportOutcome>> {
    if !enabled {
        return Ok(None);
    }

    let Some(runner_path) = discover_direct_occt_runner_with_mode(app, enabled) else {
        return Ok(None);
    };

    let runner_safe_plan = runner_supports_plan(plan);
    if !runner_safe_plan {
        return Ok(None);
    }

    let Some(plan_json) = serialize_runner_plan(plan)? else {
        return Err(AppError::validation(
            "Direct OCCT runner support gate accepted plan, but runner serialization rejected it."
                .to_string(),
        ));
    };

    let output_dir = output_dir.as_ref();
    fs::create_dir_all(output_dir).map_err(|err| {
        AppError::validation(format!(
            "Direct OCCT runner could not create output dir '{}': {}",
            output_dir.display(),
            err
        ))
    })?;

    let plan_path = output_dir.join(PLAN_FILE_NAME);
    fs::write(&plan_path, plan_json).map_err(|err| {
        AppError::validation(format!(
            "Direct OCCT runner could not write '{}': {}",
            plan_path.display(),
            err
        ))
    })?;

    let output = Command::new(&runner_path)
        .arg("--plan")
        .arg(&plan_path)
        .arg("--out")
        .arg(output_dir)
        .output()
        .map_err(|err| {
            AppError::validation(format!(
                "Direct OCCT runner could not start '{}': {}",
                runner_path.display(),
                err
            ))
        })?;

    if !output.status.success() && runner_reported_unsupported(&output) {
        // Exit 11 means the runner build itself claims an op our support gate
        // accepted is unsupported — version skew between gate and runner is a
        // bug and must surface loudly. Any other structured unsupported exit
        // is a graceful "this runner cannot take the plan": skip the runner
        // tier and let the caller fall through to the next backend.
        if output.status.code() != Some(11) {
            return Ok(None);
        }
        return Err(AppError::with_details(
            crate::models::AppErrorCode::Validation,
            "Direct OCCT runner rejected a plan that runner support gate accepted.",
            format!(
                "runner: {}\nexit: {}\nstdout: {}\nstderr: {}",
                runner_path.display(),
                output
                    .status
                    .code()
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "terminated by signal".to_string()),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        ));
    }

    if !output.status.success() {
        return Err(AppError::with_details(
            crate::models::AppErrorCode::Validation,
            "Direct OCCT runner failed.",
            format!(
                "runner: {}\nexit: {}\nstdout: {}\nstderr: {}",
                runner_path.display(),
                output
                    .status
                    .code()
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "terminated by signal".to_string()),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        ));
    }

    Ok(Some(
        super::direct_occt_sdk::NativeExportOutcome::Exported {
            step_path: output_dir.join(MODEL_STEP_FILE_NAME),
            stl_path: output_dir.join(PREVIEW_STL_FILE_NAME),
        },
    ))
}

pub(crate) fn discover_direct_occt_runner_with_mode(
    app: &dyn PathResolver,
    enabled: bool,
) -> Option<PathBuf> {
    if !enabled {
        return None;
    }

    let mut candidates = Vec::new();
    if let Some(path) = app.resource_path(RUNNER_RESOURCE_PATH) {
        candidates.push(path);
    }
    if let Some(path) = app.resource_path(LEGACY_RUNNER_RESOURCE_PATH) {
        candidates.push(path);
    }

    #[cfg(test)]
    let skip_cwd_fallbacks = test_discovery::cwd_fallbacks_disabled();
    #[cfg(not(test))]
    let skip_cwd_fallbacks = false;
    if !skip_cwd_fallbacks {
        for fallback in runner_fallback_paths() {
            candidates.push(PathBuf::from(fallback));
        }
    }

    candidates.into_iter().find(|candidate| candidate.exists())
}

fn runner_enabled() -> bool {
    match std::env::var(RUNNER_DISABLED_ENV) {
        Ok(value) => {
            let value = value.trim().to_ascii_lowercase();
            !(value == "1" || value == "true" || value == "yes" || value == "on")
        }
        Err(_) => true,
    }
}

fn runner_fallback_paths() -> &'static [&'static str] {
    if cfg!(windows) {
        &[
            "../.dist/runtime/occt/bin/direct-occt-runner.exe",
            ".dist/runtime/occt/bin/direct-occt-runner.exe",
            "../bin/direct-occt-runner.exe",
            "bin/direct-occt-runner.exe",
            "../.dist/runtime/occt/bin/direct-occt-runner",
            ".dist/runtime/occt/bin/direct-occt-runner",
            "../bin/direct-occt-runner",
            "bin/direct-occt-runner",
        ]
    } else {
        &[
            "../.dist/runtime/occt/bin/direct-occt-runner",
            ".dist/runtime/occt/bin/direct-occt-runner",
            "../bin/direct-occt-runner",
            "bin/direct-occt-runner",
            "../.dist/runtime/occt/bin/direct-occt-runner.exe",
            ".dist/runtime/occt/bin/direct-occt-runner.exe",
            "../bin/direct-occt-runner.exe",
            "bin/direct-occt-runner.exe",
        ]
    }
}

fn serialize_runner_plan(plan: &OcctPlan) -> AppResult<Option<String>> {
    let Some(plan) = runner_plan(plan)? else {
        return Ok(None);
    };
    serde_json::to_string_pretty(&plan)
        .map(Some)
        .map_err(|err| {
            AppError::validation(format!(
                "Direct OCCT runner plan serialization failed: {}",
                err
            ))
        })
}

fn runner_plan(plan: &OcctPlan) -> AppResult<Option<RunnerPlan>> {
    let mut parts = Vec::with_capacity(plan.parts.len());
    for part in &plan.parts {
        let mut commands = Vec::with_capacity(part.commands.len());
        for command in &part.commands {
            let Some(runner_command) = runner_command(command)? else {
                return Ok(None);
            };
            commands.push(runner_command);
        }
        parts.push(RunnerPart {
            key: part.key.clone(),
            label: part.label.clone(),
            root: part.root.0,
            commands,
        });
    }

    let body = RunnerPlan {
        schema_version: 1,
        plan_id: runner_plan_id(&parts)?,
        parts,
    };
    Ok(Some(body))
}

fn runner_plan_id(parts: &[RunnerPart]) -> AppResult<String> {
    let body = serde_json::json!({
        "schemaVersion": 1,
        "parts": parts,
    });
    let body = serde_json::to_vec(&body).map_err(|err| {
        AppError::validation(format!("Direct OCCT runner plan hashing failed: {}", err))
    })?;
    let mut hasher = Sha256::new();
    hasher.update(&body);
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn runner_supports_plan(plan: &OcctPlan) -> bool {
    plan.parts
        .iter()
        .all(|part| part.commands.iter().all(runner_command_supported))
}

fn runner_command_supported(command: &OcctCommand) -> bool {
    if !runner_args_supported(&command.args)
        || !runner_keywords_sources_supported(&command.keywords)
    {
        return false;
    }
    if !runner_op_supported(command.op) {
        return false;
    }
    if command.keywords.is_empty() {
        return true;
    }
    match command.op {
        OcctOp::Box => runner_box_keywords_supported(command),
        OcctOp::Profile => runner_profile_keywords_supported(command),
        OcctOp::Plane => runner_plane_keywords_supported(command),
        OcctOp::ClipBox => runner_clip_box_keywords_supported(command),
        OcctOp::Fillet | OcctOp::Chamfer => runner_exact_edge_selector_supported(command),
        OcctOp::Shell => runner_shell_supported(command),
        _ => false,
    }
}

fn runner_box_keywords_supported(command: &OcctCommand) -> bool {
    if command.args.len() != 3
        || !command
            .args
            .iter()
            .all(|arg| matches!(arg, OcctArg::Number(_)))
    {
        return false;
    }
    if command.keywords.len() != 1 {
        return false;
    }
    let keyword = &command.keywords[0];
    keyword.name == "align"
        && keyword.selector_payload().is_none()
        && runner_align_tuple_supported(keyword.source_arg())
}

fn runner_profile_keywords_supported(command: &OcctCommand) -> bool {
    if !command.args.is_empty() {
        return false;
    }
    let mut saw_outer = false;
    for keyword in &command.keywords {
        match keyword.name.as_str() {
            "outer" => {
                saw_outer = true;
                if !runner_ref_collection_supported(keyword.source_arg()) {
                    return false;
                }
            }
            "holes" => {
                if !runner_ref_collection_supported(keyword.source_arg()) {
                    return false;
                }
            }
            _ => return false,
        }
        if keyword.selector_payload().is_some() {
            return false;
        }
    }
    saw_outer
}

fn runner_clip_box_keywords_supported(command: &OcctCommand) -> bool {
    if command.args.len() != 1 || !matches!(command.args[0], OcctArg::Ref(_)) {
        return false;
    }
    let mut saw_x = false;
    let mut saw_y = false;
    let mut saw_z = false;
    for keyword in &command.keywords {
        match keyword.name.as_str() {
            "x" => {
                saw_x = true;
                if !runner_range_arg_supported(keyword.source_arg()) {
                    return false;
                }
            }
            "y" => {
                saw_y = true;
                if !runner_range_arg_supported(keyword.source_arg()) {
                    return false;
                }
            }
            "z" => {
                saw_z = true;
                if !runner_range_arg_supported(keyword.source_arg()) {
                    return false;
                }
            }
            _ => return false,
        }
        if keyword.selector_payload().is_some() {
            return false;
        }
    }
    saw_x && saw_y && saw_z
}

fn runner_plane_keywords_supported(command: &OcctCommand) -> bool {
    if !command.args.is_empty() {
        return false;
    }
    for keyword in &command.keywords {
        if keyword.selector_payload().is_some() {
            return false;
        }
        match keyword.name.as_str() {
            "origin" | "x" | "normal" => {
                if !matches!(keyword.source_arg(), OcctArg::Point3(_)) {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

fn runner_ref_collection_supported(arg: &OcctArg) -> bool {
    match arg {
        OcctArg::Ref(_) => true,
        OcctArg::List(items) => items.iter().all(|item| matches!(item, OcctArg::Ref(_))),
        _ => false,
    }
}

fn runner_range_arg_supported(arg: &OcctArg) -> bool {
    match arg {
        OcctArg::Point2(_) => true,
        OcctArg::List(items) if items.len() == 2 => {
            items.iter().all(|item| matches!(item, OcctArg::Number(_)))
        }
        _ => false,
    }
}

fn runner_align_tuple_supported(arg: &OcctArg) -> bool {
    let OcctArg::List(items) = arg else {
        return false;
    };
    if items.len() != 3 {
        return false;
    }
    items.iter().all(|item| {
        matches!(
            item,
            OcctArg::Symbol(value) | OcctArg::Text(value)
                if value == "min" || value == "center" || value == "max"
        )
    })
}

fn runner_exact_edge_selector_supported(command: &OcctCommand) -> bool {
    if command.args.len() != 2 {
        return false;
    }
    if !matches!(command.args[0], OcctArg::Number(_)) || !matches!(command.args[1], OcctArg::Ref(_))
    {
        return false;
    }
    if command.keywords.is_empty() {
        return true;
    }
    if command.keywords.len() != 1 {
        return false;
    }
    let keyword = &command.keywords[0];
    if keyword.name != "edges" {
        return false;
    }
    matches!(
        keyword.selector_payload(),
        Some(crate::ecky_core_ir::CoreSelectorPayload::EdgeAll)
            | Some(crate::ecky_core_ir::CoreSelectorPayload::EdgeTargetIds(_))
            | Some(crate::ecky_core_ir::CoreSelectorPayload::EdgeClauses(_))
    )
}

fn runner_exact_face_selector_supported(command: &OcctCommand) -> bool {
    if command.args.len() != 2 {
        return false;
    }
    if !matches!(command.args[0], OcctArg::Number(_)) || !matches!(command.args[1], OcctArg::Ref(_))
    {
        return false;
    }
    if command.keywords.len() != 1 {
        return false;
    }
    let keyword = &command.keywords[0];
    if keyword.name != "faces" {
        return false;
    }
    matches!(
        keyword.selector_payload(),
        Some(crate::ecky_core_ir::CoreSelectorPayload::FaceTargetIds(_))
            | Some(crate::ecky_core_ir::CoreSelectorPayload::FaceClauses(_))
    )
}

fn runner_shell_supported(command: &OcctCommand) -> bool {
    if command.args.len() != 2 {
        return false;
    }
    if !matches!(command.args[0], OcctArg::Number(_)) || !matches!(command.args[1], OcctArg::Ref(_))
    {
        return false;
    }
    if command.keywords.is_empty() {
        return true;
    }
    runner_exact_face_selector_supported(command)
}

fn runner_op_supported(op: OcctOp) -> bool {
    matches!(
        op,
        OcctOp::Box
            | OcctOp::Sphere
            | OcctOp::Cylinder
            | OcctOp::Cone
            | OcctOp::Circle
            | OcctOp::Rectangle
            | OcctOp::RoundedRectangle
            | OcctOp::RoundedPolygon
            | OcctOp::Polygon
            | OcctOp::Profile
            | OcctOp::MakeFace
            | OcctOp::ImportStl
            | OcctOp::Extrude
            | OcctOp::Revolve
            | OcctOp::Loft
            | OcctOp::Sweep
            | OcctOp::Twist
            | OcctOp::Taper
            | OcctOp::Offset
            | OcctOp::Path
            | OcctOp::HelixPath
            | OcctOp::BezierPath
            | OcctOp::Bspline
            | OcctOp::Plane
            | OcctOp::Location
            | OcctOp::PathFrame
            | OcctOp::Place
            | OcctOp::ClipBox
            | OcctOp::LinearArray
            | OcctOp::RadialArray
            | OcctOp::GridArray
            | OcctOp::ArcArray
            | OcctOp::Union
            | OcctOp::Difference
            | OcctOp::Intersection
            | OcctOp::Fillet
            | OcctOp::Chamfer
            | OcctOp::Shell
            | OcctOp::Translate
            | OcctOp::Rotate
            | OcctOp::Scale
            | OcctOp::Mirror
            | OcctOp::Compound
    )
}

fn runner_reported_unsupported(output: &std::process::Output) -> bool {
    let stderr = String::from_utf8_lossy(&output.stderr);
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stderr) {
        return json["class"] == "validation_error"
            && (json["code"] == "unsupported_op" || json["code"] == "unsupported_selector_form");
    }
    output.status.code() == Some(11) && stderr.contains("unsupported direct OCCT op")
}

fn runner_command(command: &OcctCommand) -> AppResult<Option<RunnerCommand>> {
    if !runner_args_supported(&command.args)
        || !runner_keywords_sources_supported(&command.keywords)
    {
        return Err(AppError::validation(
            "Direct OCCT runner requires resolved args before serialization.".to_string(),
        ));
    }
    if !runner_command_supported(command) {
        return Ok(None);
    }
    let mut keywords = Vec::with_capacity(command.keywords.len());
    for keyword in &command.keywords {
        if matches!(
            keyword.selector_payload(),
            Some(crate::ecky_core_ir::CoreSelectorPayload::EdgeAll)
        ) {
            continue;
        }
        let Some(serialized_keyword) = runner_keyword(keyword)? else {
            return Ok(None);
        };
        keywords.push(serialized_keyword);
    }

    Ok(Some(RunnerCommand {
        output: command.output.0,
        op: runner_op_token(command.op).to_string(),
        args: command
            .args
            .iter()
            .map(runner_arg)
            .collect::<AppResult<Vec<_>>>()?,
        keywords,
    }))
}

fn runner_keyword(keyword: &OcctKeyword) -> AppResult<Option<RunnerKeyword>> {
    match &keyword.value {
        super::direct_occt::OcctKeywordValue::Arg(value) => Ok(Some(RunnerKeyword {
            name: keyword.name.clone(),
            kind: "arg".to_string(),
            value: Some(runner_arg(value)?),
            payload: None,
        })),
        super::direct_occt::OcctKeywordValue::Selector { source, payload } => {
            let Some(payload) = runner_selector_payload(payload)? else {
                return Ok(None);
            };
            Ok(Some(RunnerKeyword {
                name: keyword.name.clone(),
                kind: "selector".to_string(),
                value: Some(runner_arg(source)?),
                payload: Some(payload),
            }))
        }
    }
}

fn runner_selector_payload(
    payload: &crate::ecky_core_ir::CoreSelectorPayload,
) -> AppResult<Option<serde_json::Value>> {
    let value = match payload {
        crate::ecky_core_ir::CoreSelectorPayload::EdgeAll => return Ok(None),
        crate::ecky_core_ir::CoreSelectorPayload::EdgeTargetIds(target_ids) => {
            serde_json::json!({
                "type": "targetIds",
                "kind": "edge",
                "targetIds": target_ids,
            })
        }
        crate::ecky_core_ir::CoreSelectorPayload::EdgeClauses(clauses) => serde_json::json!({
            "type": "clauses",
            "kind": "edge",
            "clauses": clauses.iter().map(runner_edge_clause).collect::<Vec<_>>(),
        }),
        crate::ecky_core_ir::CoreSelectorPayload::FaceTargetIds(target_ids) => {
            serde_json::json!({
                "type": "targetIds",
                "kind": "face",
                "targetIds": target_ids,
            })
        }
        crate::ecky_core_ir::CoreSelectorPayload::FaceClauses(clauses) => serde_json::json!({
            "type": "clauses",
            "kind": "face",
            "clauses": clauses.iter().map(runner_face_clause).collect::<Vec<_>>(),
        }),
    };

    Ok(Some(value))
}

fn runner_edge_clause(clause: &crate::ecky_core_ir::CoreEdgeSelectorClause) -> serde_json::Value {
    match clause {
        crate::ecky_core_ir::CoreEdgeSelectorClause::Axis(axis) => serde_json::json!({
            "type": "axis",
            "axis": runner_edge_axis(axis),
        }),
        crate::ecky_core_ir::CoreEdgeSelectorClause::Boundary { axis, bound } => {
            serde_json::json!({
                "type": "boundary",
                "axis": runner_edge_axis(axis),
                "bound": runner_edge_bound(bound),
            })
        }
    }
}

fn runner_face_clause(clause: &crate::ecky_core_ir::CoreFaceSelectorClause) -> serde_json::Value {
    match clause {
        crate::ecky_core_ir::CoreFaceSelectorClause::Boundary { axis, bound } => {
            serde_json::json!({
                "type": "boundary",
                "axis": runner_edge_axis(axis),
                "bound": runner_edge_bound(bound),
            })
        }
        crate::ecky_core_ir::CoreFaceSelectorClause::Planar => serde_json::json!({
            "type": "planar",
        }),
        crate::ecky_core_ir::CoreFaceSelectorClause::Normal(axis) => serde_json::json!({
            "type": "normal",
            "axis": runner_edge_axis(axis),
        }),
        crate::ecky_core_ir::CoreFaceSelectorClause::Area(rank) => serde_json::json!({
            "type": "area",
            "rank": runner_face_area_rank(rank),
        }),
    }
}

fn runner_edge_axis(axis: &crate::ecky_core_ir::CoreEdgeAxis) -> &'static str {
    match axis {
        crate::ecky_core_ir::CoreEdgeAxis::X => "x",
        crate::ecky_core_ir::CoreEdgeAxis::Y => "y",
        crate::ecky_core_ir::CoreEdgeAxis::Z => "z",
    }
}

fn runner_edge_bound(bound: &crate::ecky_core_ir::CoreEdgeBound) -> &'static str {
    match bound {
        crate::ecky_core_ir::CoreEdgeBound::Min => "min",
        crate::ecky_core_ir::CoreEdgeBound::Max => "max",
    }
}

fn runner_face_area_rank(rank: &crate::ecky_core_ir::CoreFaceAreaRank) -> &'static str {
    match rank {
        crate::ecky_core_ir::CoreFaceAreaRank::Min => "min",
        crate::ecky_core_ir::CoreFaceAreaRank::Max => "max",
    }
}

fn runner_arg(arg: &OcctArg) -> AppResult<RunnerArg> {
    Ok(match arg {
        OcctArg::Number(value) => RunnerArg {
            kind: "number".to_string(),
            value: serde_json::json!(value),
        },
        OcctArg::Boolean(value) => RunnerArg {
            kind: "boolean".to_string(),
            value: serde_json::json!(value),
        },
        OcctArg::Text(value) => RunnerArg {
            kind: "text".to_string(),
            value: serde_json::json!(value),
        },
        OcctArg::Symbol(value) => RunnerArg {
            kind: "symbol".to_string(),
            value: serde_json::json!(value),
        },
        OcctArg::Point2(value) => RunnerArg {
            kind: "point2".to_string(),
            value: serde_json::json!(value),
        },
        OcctArg::Point3(value) => RunnerArg {
            kind: "point3".to_string(),
            value: serde_json::json!(value),
        },
        OcctArg::List(values) => RunnerArg {
            kind: "list".to_string(),
            value: serde_json::Value::Array(
                values
                    .iter()
                    .map(runner_arg)
                    .collect::<AppResult<Vec<_>>>()?
                    .into_iter()
                    .map(|item| serde_json::json!(item))
                    .collect(),
            ),
        },
        OcctArg::Param(value) => {
            return Err(AppError::validation(format!(
                "Direct OCCT runner requires resolved args; unresolved param `{value}` reached runner serialization."
            )));
        }
        OcctArg::Ref(value) => RunnerArg {
            kind: "ref".to_string(),
            value: serde_json::json!(value.0),
        },
    })
}

fn runner_args_supported(args: &[OcctArg]) -> bool {
    args.iter().all(runner_arg_supported)
}

fn runner_arg_supported(arg: &OcctArg) -> bool {
    match arg {
        OcctArg::Param(_) => false,
        OcctArg::List(items) => items.iter().all(runner_arg_supported),
        _ => true,
    }
}

fn runner_keywords_sources_supported(keywords: &[OcctKeyword]) -> bool {
    keywords
        .iter()
        .all(|keyword| runner_arg_supported(keyword.source_arg()))
}

fn runner_op_token(op: OcctOp) -> &'static str {
    match op {
        OcctOp::Box => "box",
        OcctOp::Sphere => "sphere",
        OcctOp::Cylinder => "cylinder",
        OcctOp::Cone => "cone",
        OcctOp::Circle => "circle",
        OcctOp::Rectangle => "rectangle",
        OcctOp::RoundedRectangle => "rounded-rect",
        OcctOp::RoundedPolygon => "rounded-polygon",
        OcctOp::Polygon => "polygon",
        OcctOp::Profile => "profile",
        OcctOp::MakeFace => "make-face",
        OcctOp::ImportStl => "import-stl",
        OcctOp::Extrude => "extrude",
        OcctOp::Revolve => "revolve",
        OcctOp::Loft => "loft",
        OcctOp::Sweep => "sweep",
        OcctOp::Twist => "twist",
        OcctOp::Taper => "taper",
        OcctOp::Offset => "offset",
        OcctOp::Path => "path",
        OcctOp::HelixPath => "helix-path",
        OcctOp::BezierPath => "bezier-path",
        OcctOp::Bspline => "bspline",
        OcctOp::Plane => "plane",
        OcctOp::Location => "location",
        OcctOp::PathFrame => "path-frame",
        OcctOp::Place => "place",
        OcctOp::ClipBox => "clip-box",
        OcctOp::LinearArray => "linear-array",
        OcctOp::RadialArray => "radial-array",
        OcctOp::GridArray => "grid-array",
        OcctOp::ArcArray => "arc-array",
        OcctOp::Union => "union",
        OcctOp::Difference => "difference",
        OcctOp::Intersection => "intersection",
        OcctOp::Fillet => "fillet",
        OcctOp::Chamfer => "chamfer",
        OcctOp::Shell => "shell",
        OcctOp::Translate => "translate",
        OcctOp::Rotate => "rotate",
        OcctOp::Scale => "scale",
        OcctOp::Mirror => "mirror",
        OcctOp::Compound => "compound",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecky_cad_host::direct_occt::{
        OcctArg, OcctCommand, OcctKeyword, OcctKeywordValue, OcctOp, OcctParameter,
        OcctParameterKind, OcctPartPlan, OcctPlan, OcctSlot,
    };
    use crate::ecky_core_ir::{
        CoreEdgeAxis, CoreEdgeBound, CoreFaceAreaRank, CoreFaceSelectorClause, CoreSelectorPayload,
    };
    use crate::models::PathResolver;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;

    struct TestResolver {
        root: PathBuf,
    }

    impl PathResolver for TestResolver {
        fn app_config_dir(&self) -> PathBuf {
            self.root.join("config")
        }

        fn app_data_dir(&self) -> PathBuf {
            self.root.join("data")
        }

        fn resource_path(&self, path: &str) -> Option<PathBuf> {
            let candidate = self.root.join("resources").join(path);
            candidate.exists().then_some(candidate)
        }
    }

    fn temp_root(label: &str) -> PathBuf {
        let unique = format!(
            "{}-{}-{}",
            label,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        );
        std::env::temp_dir().join(format!("ecky-direct-occt-runner-{}", unique))
    }

    fn write_executable(path: &Path, contents: &str) {
        fs::write(path, contents).expect("write script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(path).expect("metadata").permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(path, permissions).expect("chmod");
        }
    }

    fn run_real_runner_plan_json(
        label: &str,
        plan: &OcctPlan,
    ) -> Option<(PathBuf, serde_json::Value)> {
        let root = temp_root(label);
        let resolver = TestResolver { root: root.clone() };
        let runner = discover_direct_occt_runner_with_mode(&resolver, true)?;
        if !runner.is_file() {
            return None;
        }

        let output_dir = root.join("bundle");
        fs::create_dir_all(&output_dir).expect("output dir");
        let plan_json = serialize_runner_plan(plan)
            .expect("plan serialization")
            .expect("runner plan");
        let plan_path = output_dir.join(PLAN_FILE_NAME);
        fs::write(&plan_path, plan_json).expect("write plan");

        let output = std::process::Command::new(&runner)
            .arg("--plan")
            .arg(&plan_path)
            .arg("--out")
            .arg(&output_dir)
            .output()
            .expect("start runner");
        assert!(
            output.status.success(),
            "runner failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let topology = serde_json::from_str(
            &fs::read_to_string(output_dir.join("topology.json")).expect("read topology"),
        )
        .expect("parse topology");
        Some((root, topology))
    }

    fn run_real_runner_plan_text(
        label: &str,
        plan_text: &str,
    ) -> Option<(PathBuf, std::process::Output)> {
        let root = temp_root(label);
        let resolver = TestResolver { root: root.clone() };
        let runner = discover_direct_occt_runner_with_mode(&resolver, true)?;
        if !runner.is_file() {
            return None;
        }

        let output_dir = root.join("bundle");
        fs::create_dir_all(&output_dir).expect("output dir");
        let plan_path = output_dir.join(PLAN_FILE_NAME);
        fs::write(&plan_path, plan_text).expect("write plan");

        let output = std::process::Command::new(&runner)
            .arg("--plan")
            .arg(&plan_path)
            .arg("--out")
            .arg(&output_dir)
            .output()
            .expect("start runner");
        Some((root, output))
    }

    fn sample_plan() -> OcctPlan {
        OcctPlan {
            parameters: vec![OcctParameter {
                key: "width".to_string(),
                kind: OcctParameterKind::Number,
            }],
            parts: vec![OcctPartPlan {
                key: "body".to_string(),
                label: "Body".to_string(),
                root: OcctSlot(1),
                commands: vec![OcctCommand {
                    output: OcctSlot(1),
                    op: OcctOp::RoundedRectangle,
                    args: vec![OcctArg::Number(12.0), OcctArg::Param("width".to_string())],
                    keywords: vec![OcctKeyword {
                        name: "faces".to_string(),
                        value: OcctKeywordValue::Selector {
                            source: OcctArg::Ref(OcctSlot(1)),
                            payload: CoreSelectorPayload::FaceClauses(vec![
                                CoreFaceSelectorClause::Planar,
                                CoreFaceSelectorClause::Normal(CoreEdgeAxis::Z),
                                CoreFaceSelectorClause::Area(CoreFaceAreaRank::Max),
                                CoreFaceSelectorClause::Boundary {
                                    axis: CoreEdgeAxis::X,
                                    bound: CoreEdgeBound::Min,
                                },
                            ]),
                        },
                    }],
                }],
            }],
        }
    }

    fn unsupported_resolved_selector_plan() -> OcctPlan {
        OcctPlan {
            parameters: Vec::new(),
            parts: vec![OcctPartPlan {
                key: "body".to_string(),
                label: "Body".to_string(),
                root: OcctSlot(1),
                commands: vec![OcctCommand {
                    output: OcctSlot(1),
                    op: OcctOp::RoundedRectangle,
                    args: vec![OcctArg::Number(12.0), OcctArg::Number(24.0)],
                    keywords: vec![OcctKeyword {
                        name: "faces".to_string(),
                        value: OcctKeywordValue::Selector {
                            source: OcctArg::Ref(OcctSlot(1)),
                            payload: CoreSelectorPayload::FaceClauses(vec![
                                CoreFaceSelectorClause::Planar,
                                CoreFaceSelectorClause::Normal(CoreEdgeAxis::Z),
                                CoreFaceSelectorClause::Area(CoreFaceAreaRank::Max),
                                CoreFaceSelectorClause::Boundary {
                                    axis: CoreEdgeAxis::X,
                                    bound: CoreEdgeBound::Min,
                                },
                            ]),
                        },
                    }],
                }],
            }],
        }
    }

    fn supported_sample_plan() -> OcctPlan {
        sample_plan_for_command(OcctCommand {
            output: OcctSlot(1),
            op: OcctOp::Box,
            args: vec![
                OcctArg::Number(12.0),
                OcctArg::Number(8.0),
                OcctArg::Number(4.0),
            ],
            keywords: Vec::new(),
        })
    }

    fn expanded_transform_plan() -> OcctPlan {
        sample_plan_for_commands(
            OcctSlot(4),
            vec![
                OcctCommand {
                    output: OcctSlot(1),
                    op: OcctOp::Box,
                    args: vec![
                        OcctArg::Number(4.0),
                        OcctArg::Number(5.0),
                        OcctArg::Number(6.0),
                    ],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(2),
                    op: OcctOp::Rotate,
                    args: vec![
                        OcctArg::Number(10.0),
                        OcctArg::Number(20.0),
                        OcctArg::Number(30.0),
                        OcctArg::Ref(OcctSlot(1)),
                    ],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(3),
                    op: OcctOp::Scale,
                    args: vec![
                        OcctArg::Number(1.1),
                        OcctArg::Number(1.2),
                        OcctArg::Number(1.0),
                        OcctArg::Ref(OcctSlot(2)),
                    ],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(4),
                    op: OcctOp::Mirror,
                    args: vec![
                        OcctArg::Text("x".to_string()),
                        OcctArg::Number(0.0),
                        OcctArg::Ref(OcctSlot(3)),
                    ],
                    keywords: Vec::new(),
                },
            ],
        )
    }

    fn expanded_array_plan() -> OcctPlan {
        sample_plan_for_commands(
            OcctSlot(6),
            vec![
                OcctCommand {
                    output: OcctSlot(1),
                    op: OcctOp::Box,
                    args: vec![
                        OcctArg::Number(1.0),
                        OcctArg::Number(1.0),
                        OcctArg::Number(1.0),
                    ],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(2),
                    op: OcctOp::LinearArray,
                    args: vec![
                        OcctArg::Number(3.0),
                        OcctArg::Number(2.0),
                        OcctArg::Number(0.0),
                        OcctArg::Number(0.0),
                        OcctArg::Ref(OcctSlot(1)),
                    ],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(3),
                    op: OcctOp::GridArray,
                    args: vec![
                        OcctArg::Number(2.0),
                        OcctArg::Number(2.0),
                        OcctArg::Number(3.0),
                        OcctArg::Number(3.0),
                        OcctArg::Ref(OcctSlot(1)),
                    ],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(4),
                    op: OcctOp::RadialArray,
                    args: vec![
                        OcctArg::Number(3.0),
                        OcctArg::Number(45.0),
                        OcctArg::Number(6.0),
                        OcctArg::Ref(OcctSlot(1)),
                    ],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(5),
                    op: OcctOp::ArcArray,
                    args: vec![
                        OcctArg::Number(3.0),
                        OcctArg::Number(8.0),
                        OcctArg::Number(0.0),
                        OcctArg::Number(90.0),
                        OcctArg::Ref(OcctSlot(1)),
                    ],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(6),
                    op: OcctOp::Compound,
                    args: vec![
                        OcctArg::Ref(OcctSlot(2)),
                        OcctArg::Ref(OcctSlot(3)),
                        OcctArg::Ref(OcctSlot(4)),
                        OcctArg::Ref(OcctSlot(5)),
                    ],
                    keywords: Vec::new(),
                },
            ],
        )
    }

    fn expanded_profile_surface_plan() -> OcctPlan {
        sample_plan_for_commands(
            OcctSlot(13),
            vec![
                OcctCommand {
                    output: OcctSlot(1),
                    op: OcctOp::RoundedRectangle,
                    args: vec![
                        OcctArg::Number(5.0),
                        OcctArg::Number(4.0),
                        OcctArg::Number(0.5),
                    ],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(2),
                    op: OcctOp::Extrude,
                    args: vec![OcctArg::Ref(OcctSlot(1)), OcctArg::Number(2.0)],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(3),
                    op: OcctOp::RoundedPolygon,
                    args: vec![
                        OcctArg::List(vec![
                            OcctArg::Point2([-2.0, -1.0]),
                            OcctArg::Point2([2.0, -1.0]),
                            OcctArg::Point2([2.0, 1.0]),
                            OcctArg::Point2([-2.0, 1.0]),
                        ]),
                        OcctArg::Number(0.2),
                    ],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(4),
                    op: OcctOp::Taper,
                    args: vec![
                        OcctArg::Number(3.0),
                        OcctArg::Number(0.7),
                        OcctArg::Ref(OcctSlot(3)),
                    ],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(5),
                    op: OcctOp::Circle,
                    args: vec![OcctArg::Number(0.5)],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(6),
                    op: OcctOp::Rectangle,
                    args: vec![OcctArg::Number(1.2), OcctArg::Number(1.2)],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(7),
                    op: OcctOp::Loft,
                    args: vec![
                        OcctArg::Number(2.0),
                        OcctArg::Ref(OcctSlot(5)),
                        OcctArg::Ref(OcctSlot(6)),
                    ],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(8),
                    op: OcctOp::BezierPath,
                    args: vec![
                        OcctArg::Point3([0.0, 0.0, 0.0]),
                        OcctArg::Point3([1.0, 0.0, 1.0]),
                        OcctArg::Point3([2.0, 0.0, 1.0]),
                        OcctArg::Point3([3.0, 0.0, 0.0]),
                    ],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(9),
                    op: OcctOp::Circle,
                    args: vec![OcctArg::Number(0.2)],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(10),
                    op: OcctOp::Sweep,
                    args: vec![OcctArg::Ref(OcctSlot(9)), OcctArg::Ref(OcctSlot(8))],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(11),
                    op: OcctOp::Bspline,
                    args: vec![OcctArg::List(vec![
                        OcctArg::Point2([-1.0, -0.5]),
                        OcctArg::Point2([0.0, -1.0]),
                        OcctArg::Point2([1.0, -0.5]),
                        OcctArg::Point2([1.0, 0.5]),
                        OcctArg::Point2([0.0, 1.0]),
                        OcctArg::Point2([-1.0, 0.5]),
                    ])],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(12),
                    op: OcctOp::Extrude,
                    args: vec![OcctArg::Ref(OcctSlot(11)), OcctArg::Number(1.0)],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(13),
                    op: OcctOp::Compound,
                    args: vec![
                        OcctArg::Ref(OcctSlot(2)),
                        OcctArg::Ref(OcctSlot(4)),
                        OcctArg::Ref(OcctSlot(7)),
                        OcctArg::Ref(OcctSlot(10)),
                        OcctArg::Ref(OcctSlot(12)),
                    ],
                    keywords: Vec::new(),
                },
            ],
        )
    }

    fn expanded_revolve_plan() -> OcctPlan {
        sample_plan_for_commands(
            OcctSlot(3),
            vec![
                OcctCommand {
                    output: OcctSlot(1),
                    op: OcctOp::Rectangle,
                    args: vec![OcctArg::Number(0.6), OcctArg::Number(1.0)],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(2),
                    op: OcctOp::Translate,
                    args: vec![
                        OcctArg::Number(2.0),
                        OcctArg::Number(0.0),
                        OcctArg::Number(0.0),
                        OcctArg::Ref(OcctSlot(1)),
                    ],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(3),
                    op: OcctOp::Revolve,
                    args: vec![OcctArg::Ref(OcctSlot(2)), OcctArg::Number(120.0)],
                    keywords: Vec::new(),
                },
            ],
        )
    }

    fn expanded_profile_offset_twist_plan() -> OcctPlan {
        sample_plan_for_commands(
            OcctSlot(8),
            vec![
                OcctCommand {
                    output: OcctSlot(1),
                    op: OcctOp::Circle,
                    args: vec![OcctArg::Number(1.0)],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(2),
                    op: OcctOp::Profile,
                    args: vec![OcctArg::Ref(OcctSlot(1))],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(3),
                    op: OcctOp::Extrude,
                    args: vec![OcctArg::Ref(OcctSlot(2)), OcctArg::Number(1.0)],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(4),
                    op: OcctOp::Rectangle,
                    args: vec![OcctArg::Number(1.0), OcctArg::Number(1.0)],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(5),
                    op: OcctOp::Offset,
                    args: vec![OcctArg::Number(0.25), OcctArg::Ref(OcctSlot(4))],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(6),
                    op: OcctOp::Extrude,
                    args: vec![OcctArg::Ref(OcctSlot(5)), OcctArg::Number(1.0)],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(7),
                    op: OcctOp::Twist,
                    args: vec![
                        OcctArg::Number(2.0),
                        OcctArg::Number(120.0),
                        OcctArg::Ref(OcctSlot(4)),
                    ],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(8),
                    op: OcctOp::Compound,
                    args: vec![
                        OcctArg::Ref(OcctSlot(3)),
                        OcctArg::Ref(OcctSlot(6)),
                        OcctArg::Ref(OcctSlot(7)),
                    ],
                    keywords: Vec::new(),
                },
            ],
        )
    }

    fn sample_plan_for_command(command: OcctCommand) -> OcctPlan {
        sample_plan_for_commands(command.output, vec![command])
    }

    fn sample_plan_for_commands(root: OcctSlot, commands: Vec<OcctCommand>) -> OcctPlan {
        OcctPlan {
            parameters: Vec::new(),
            parts: vec![OcctPartPlan {
                key: "body".to_string(),
                label: "Body".to_string(),
                root,
                commands,
            }],
        }
    }

    fn keyword_free_plane_plan() -> OcctPlan {
        sample_plan_for_command(OcctCommand {
            output: OcctSlot(1),
            op: OcctOp::Plane,
            args: Vec::new(),
            keywords: Vec::new(),
        })
    }

    fn keyworded_plane_plan() -> OcctPlan {
        sample_plan_for_command(OcctCommand {
            output: OcctSlot(1),
            op: OcctOp::Plane,
            args: Vec::new(),
            keywords: vec![
                OcctKeyword {
                    name: "origin".to_string(),
                    value: OcctKeywordValue::Arg(OcctArg::Point3([0.0, 0.0, 0.0])),
                },
                OcctKeyword {
                    name: "normal".to_string(),
                    value: OcctKeywordValue::Arg(OcctArg::Point3([0.0, 0.0, 1.0])),
                },
            ],
        })
    }

    fn supported_box_with_keyword_plan() -> OcctPlan {
        sample_plan_for_command(OcctCommand {
            output: OcctSlot(1),
            op: OcctOp::Box,
            args: vec![
                OcctArg::Number(12.0),
                OcctArg::Number(8.0),
                OcctArg::Number(4.0),
            ],
            keywords: vec![OcctKeyword {
                name: "align".to_string(),
                value: OcctKeywordValue::Arg(OcctArg::List(vec![
                    OcctArg::Symbol("min".to_string()),
                    OcctArg::Symbol("center".to_string()),
                    OcctArg::Symbol("max".to_string()),
                ])),
            }],
        })
    }

    fn keyword_profile_holes_plan() -> OcctPlan {
        sample_plan_for_commands(
            OcctSlot(4),
            vec![
                OcctCommand {
                    output: OcctSlot(1),
                    op: OcctOp::Circle,
                    args: vec![OcctArg::Number(10.0)],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(2),
                    op: OcctOp::Circle,
                    args: vec![OcctArg::Number(3.0)],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(3),
                    op: OcctOp::Profile,
                    args: Vec::new(),
                    keywords: vec![
                        OcctKeyword::arg("outer".to_string(), OcctArg::Ref(OcctSlot(1))),
                        OcctKeyword::arg(
                            "holes".to_string(),
                            OcctArg::List(vec![OcctArg::Ref(OcctSlot(2))]),
                        ),
                    ],
                },
                OcctCommand {
                    output: OcctSlot(4),
                    op: OcctOp::Extrude,
                    args: vec![OcctArg::Ref(OcctSlot(3)), OcctArg::Number(4.0)],
                    keywords: Vec::new(),
                },
            ],
        )
    }

    fn keyword_clip_box_plan() -> OcctPlan {
        sample_plan_for_commands(
            OcctSlot(2),
            vec![
                OcctCommand {
                    output: OcctSlot(1),
                    op: OcctOp::Box,
                    args: vec![
                        OcctArg::Number(20.0),
                        OcctArg::Number(20.0),
                        OcctArg::Number(20.0),
                    ],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(2),
                    op: OcctOp::ClipBox,
                    args: vec![OcctArg::Ref(OcctSlot(1))],
                    keywords: vec![
                        OcctKeyword::arg(
                            "x".to_string(),
                            OcctArg::List(vec![OcctArg::Number(0.0), OcctArg::Number(10.0)]),
                        ),
                        OcctKeyword::arg(
                            "y".to_string(),
                            OcctArg::List(vec![OcctArg::Number(-5.0), OcctArg::Number(5.0)]),
                        ),
                        OcctKeyword::arg(
                            "z".to_string(),
                            OcctArg::List(vec![OcctArg::Number(0.0), OcctArg::Number(12.0)]),
                        ),
                    ],
                },
            ],
        )
    }

    fn exact_fillet_plan() -> OcctPlan {
        sample_plan_for_commands(
            OcctSlot(2),
            vec![
                OcctCommand {
                    output: OcctSlot(1),
                    op: OcctOp::Box,
                    args: vec![
                        OcctArg::Number(20.0),
                        OcctArg::Number(20.0),
                        OcctArg::Number(10.0),
                    ],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(2),
                    op: OcctOp::Fillet,
                    args: vec![OcctArg::Number(1.5), OcctArg::Ref(OcctSlot(1))],
                    keywords: vec![OcctKeyword::selector(
                        "edges".to_string(),
                        OcctArg::Text("target-id:body:edge:-10--10-0_10--10-0".to_string()),
                        CoreSelectorPayload::EdgeTargetIds(vec![
                            "body:edge:-10--10-0_10--10-0".to_string()
                        ]),
                    )],
                },
            ],
        )
    }

    fn exact_chamfer_plan() -> OcctPlan {
        sample_plan_for_commands(
            OcctSlot(2),
            vec![
                OcctCommand {
                    output: OcctSlot(1),
                    op: OcctOp::Box,
                    args: vec![
                        OcctArg::Number(20.0),
                        OcctArg::Number(20.0),
                        OcctArg::Number(10.0),
                    ],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(2),
                    op: OcctOp::Chamfer,
                    args: vec![OcctArg::Number(1.25), OcctArg::Ref(OcctSlot(1))],
                    keywords: vec![OcctKeyword::selector(
                        "edges".to_string(),
                        OcctArg::Text("target-id:body:edge:-10--10-0_10--10-0".to_string()),
                        CoreSelectorPayload::EdgeTargetIds(vec![
                            "body:edge:-10--10-0_10--10-0".to_string()
                        ]),
                    )],
                },
            ],
        )
    }

    fn clause_fillet_plan() -> OcctPlan {
        sample_plan_for_commands(
            OcctSlot(2),
            vec![
                OcctCommand {
                    output: OcctSlot(1),
                    op: OcctOp::Box,
                    args: vec![
                        OcctArg::Number(20.0),
                        OcctArg::Number(20.0),
                        OcctArg::Number(10.0),
                    ],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(2),
                    op: OcctOp::Fillet,
                    args: vec![OcctArg::Number(1.5), OcctArg::Ref(OcctSlot(1))],
                    keywords: vec![OcctKeyword::selector(
                        "edges".to_string(),
                        OcctArg::Text("left+vertical".to_string()),
                        CoreSelectorPayload::EdgeClauses(vec![
                            crate::ecky_core_ir::CoreEdgeSelectorClause::Boundary {
                                axis: crate::ecky_core_ir::CoreEdgeAxis::X,
                                bound: crate::ecky_core_ir::CoreEdgeBound::Min,
                            },
                            crate::ecky_core_ir::CoreEdgeSelectorClause::Axis(
                                crate::ecky_core_ir::CoreEdgeAxis::Z,
                            ),
                        ]),
                    )],
                },
            ],
        )
    }

    fn clause_chamfer_plan() -> OcctPlan {
        sample_plan_for_commands(
            OcctSlot(2),
            vec![
                OcctCommand {
                    output: OcctSlot(1),
                    op: OcctOp::Box,
                    args: vec![
                        OcctArg::Number(20.0),
                        OcctArg::Number(20.0),
                        OcctArg::Number(10.0),
                    ],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(2),
                    op: OcctOp::Chamfer,
                    args: vec![OcctArg::Number(1.25), OcctArg::Ref(OcctSlot(1))],
                    keywords: vec![OcctKeyword::selector(
                        "edges".to_string(),
                        OcctArg::Text("left+vertical".to_string()),
                        CoreSelectorPayload::EdgeClauses(vec![
                            crate::ecky_core_ir::CoreEdgeSelectorClause::Boundary {
                                axis: crate::ecky_core_ir::CoreEdgeAxis::X,
                                bound: crate::ecky_core_ir::CoreEdgeBound::Min,
                            },
                            crate::ecky_core_ir::CoreEdgeSelectorClause::Axis(
                                crate::ecky_core_ir::CoreEdgeAxis::Z,
                            ),
                        ]),
                    )],
                },
            ],
        )
    }

    fn edge_all_fillet_plan() -> OcctPlan {
        sample_plan_for_commands(
            OcctSlot(2),
            vec![
                OcctCommand {
                    output: OcctSlot(1),
                    op: OcctOp::Box,
                    args: vec![
                        OcctArg::Number(20.0),
                        OcctArg::Number(20.0),
                        OcctArg::Number(10.0),
                    ],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(2),
                    op: OcctOp::Fillet,
                    args: vec![OcctArg::Number(1.5), OcctArg::Ref(OcctSlot(1))],
                    keywords: vec![OcctKeyword::selector(
                        "edges".to_string(),
                        OcctArg::Text("all".to_string()),
                        CoreSelectorPayload::EdgeAll,
                    )],
                },
            ],
        )
    }

    fn edge_all_chamfer_plan() -> OcctPlan {
        sample_plan_for_commands(
            OcctSlot(2),
            vec![
                OcctCommand {
                    output: OcctSlot(1),
                    op: OcctOp::Box,
                    args: vec![
                        OcctArg::Number(20.0),
                        OcctArg::Number(20.0),
                        OcctArg::Number(10.0),
                    ],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(2),
                    op: OcctOp::Chamfer,
                    args: vec![OcctArg::Number(1.25), OcctArg::Ref(OcctSlot(1))],
                    keywords: vec![OcctKeyword::selector(
                        "edges".to_string(),
                        OcctArg::Text("all".to_string()),
                        CoreSelectorPayload::EdgeAll,
                    )],
                },
            ],
        )
    }

    fn exact_shell_plan() -> OcctPlan {
        sample_plan_for_commands(
            OcctSlot(2),
            vec![
                OcctCommand {
                    output: OcctSlot(1),
                    op: OcctOp::Box,
                    args: vec![
                        OcctArg::Number(20.0),
                        OcctArg::Number(20.0),
                        OcctArg::Number(10.0),
                    ],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(2),
                    op: OcctOp::Shell,
                    args: vec![OcctArg::Number(1.0), OcctArg::Ref(OcctSlot(1))],
                    keywords: vec![OcctKeyword::selector(
                        "faces".to_string(),
                        OcctArg::Text("target-id:body:face:0-0-10:400".to_string()),
                        CoreSelectorPayload::FaceTargetIds(
                            vec!["body:face:0-0-10:400".to_string()],
                        ),
                    )],
                },
            ],
        )
    }

    fn shell_clause_plan() -> OcctPlan {
        sample_plan_for_commands(
            OcctSlot(2),
            vec![
                OcctCommand {
                    output: OcctSlot(1),
                    op: OcctOp::Box,
                    args: vec![
                        OcctArg::Number(20.0),
                        OcctArg::Number(20.0),
                        OcctArg::Number(10.0),
                    ],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(2),
                    op: OcctOp::Shell,
                    args: vec![OcctArg::Number(1.0), OcctArg::Ref(OcctSlot(1))],
                    keywords: vec![OcctKeyword::selector(
                        "faces".to_string(),
                        OcctArg::Text("faces:[planar normal:z area:max]".to_string()),
                        CoreSelectorPayload::FaceClauses(vec![
                            CoreFaceSelectorClause::Planar,
                            CoreFaceSelectorClause::Normal(CoreEdgeAxis::Z),
                            CoreFaceSelectorClause::Area(CoreFaceAreaRank::Max),
                        ]),
                    )],
                },
            ],
        )
    }

    fn shell_plan() -> OcctPlan {
        sample_plan_for_commands(
            OcctSlot(2),
            vec![
                OcctCommand {
                    output: OcctSlot(1),
                    op: OcctOp::Box,
                    args: vec![
                        OcctArg::Number(20.0),
                        OcctArg::Number(20.0),
                        OcctArg::Number(10.0),
                    ],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(2),
                    op: OcctOp::Shell,
                    args: vec![OcctArg::Number(1.0), OcctArg::Ref(OcctSlot(1))],
                    keywords: Vec::new(),
                },
            ],
        )
    }

    #[test]
    fn runner_support_gate_matches_proven_subset() {
        let cases = [
            (OcctOp::Box, true),
            (OcctOp::Sphere, true),
            (OcctOp::Cylinder, true),
            (OcctOp::Cone, true),
            (OcctOp::Circle, true),
            (OcctOp::Rectangle, true),
            (OcctOp::RoundedRectangle, true),
            (OcctOp::RoundedPolygon, true),
            (OcctOp::Polygon, true),
            (OcctOp::Profile, true),
            (OcctOp::MakeFace, true),
            (OcctOp::ImportStl, true),
            (OcctOp::Extrude, true),
            (OcctOp::Revolve, true),
            (OcctOp::Loft, true),
            (OcctOp::Sweep, true),
            (OcctOp::Twist, true),
            (OcctOp::Taper, true),
            (OcctOp::Offset, true),
            (OcctOp::Path, true),
            (OcctOp::HelixPath, true),
            (OcctOp::BezierPath, true),
            (OcctOp::Bspline, true),
            (OcctOp::Plane, true),
            (OcctOp::Location, true),
            (OcctOp::PathFrame, true),
            (OcctOp::Place, true),
            (OcctOp::ClipBox, true),
            (OcctOp::LinearArray, true),
            (OcctOp::RadialArray, true),
            (OcctOp::GridArray, true),
            (OcctOp::ArcArray, true),
            (OcctOp::Union, true),
            (OcctOp::Difference, true),
            (OcctOp::Intersection, true),
            (OcctOp::Fillet, true),
            (OcctOp::Chamfer, true),
            (OcctOp::Shell, true),
            (OcctOp::Translate, true),
            (OcctOp::Rotate, true),
            (OcctOp::Scale, true),
            (OcctOp::Mirror, true),
            (OcctOp::Compound, true),
        ];

        for (op, supported) in cases {
            assert_eq!(
                runner_op_supported(op),
                supported,
                "runner support gate for {}",
                runner_op_token(op)
            );
        }
    }

    #[test]
    fn runner_supports_plan_rejects_keywords_even_on_supported_ops() {
        assert!(runner_supports_plan(&supported_sample_plan()));
        assert!(!runner_supports_plan(&sample_plan()));
    }

    #[test]
    fn runner_supports_plan_accepts_supported_keyword_profile_and_clip_box_forms() {
        assert!(runner_supports_plan(&supported_box_with_keyword_plan()));
        assert!(runner_supports_plan(&keyworded_plane_plan()));
        assert!(runner_supports_plan(&keyword_profile_holes_plan()));
        assert!(runner_supports_plan(&keyword_clip_box_plan()));
    }

    #[test]
    fn runner_supports_plan_accepts_exact_selector_forms() {
        assert!(runner_supports_plan(&shell_plan()));
        assert!(runner_supports_plan(&edge_all_fillet_plan()));
        assert!(runner_supports_plan(&edge_all_chamfer_plan()));
        assert!(runner_supports_plan(&exact_fillet_plan()));
        assert!(runner_supports_plan(&exact_chamfer_plan()));
        assert!(runner_supports_plan(&clause_fillet_plan()));
        assert!(runner_supports_plan(&clause_chamfer_plan()));
        assert!(runner_supports_plan(&exact_shell_plan()));
        assert!(runner_supports_plan(&shell_clause_plan()));
    }

    #[test]
    fn runner_supports_helical_ridge_plan() {
        let program = crate::ecky_scheme::compile_to_core_program(
            r#"
            (model
              (part body
                (helical-ridge
                  :radius 20
                  :pitch 6
                  :height 30
                  :base-width 2
                  :crest-width 1
                  :depth 1.5)))
            "#,
        )
        .expect("program");
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");
        assert!(
            runner_supports_plan(&plan),
            "helical-ridge plan must be runner-safe"
        );
    }

    #[test]
    fn runner_supports_plan_accepts_keyword_free_frame_ops() {
        let cases = [
            OcctOp::Plane,
            OcctOp::Location,
            OcctOp::PathFrame,
            OcctOp::Place,
        ];

        for op in cases {
            let plan = sample_plan_for_command(OcctCommand {
                output: OcctSlot(1),
                op,
                args: Vec::new(),
                keywords: Vec::new(),
            });
            assert!(
                runner_supports_plan(&plan),
                "runner support gate for {}",
                runner_op_token(op)
            );
        }
    }

    #[test]
    fn supported_runner_plan_rejection_is_hard_error_not_fallback() {
        let root = temp_root("runner-supported-plan-rejection");
        fs::create_dir_all(
            root.join("resources")
                .join("runtime")
                .join("occt")
                .join("bin"),
        )
        .expect("runner dir");
        let runner = root
            .join("resources")
            .join("runtime")
            .join("occt")
            .join("bin")
            .join("direct-occt-runner");
        write_executable(
            &runner,
            r#"#!/bin/sh
if [ "${1:-}" = "--version" ]; then
  echo "direct-occt-runner 0.1.0"
  exit 0
fi
echo '{"class":"validation_error","code":"unsupported_op","message":"forced unsupported","details":"boom"}' >&2
exit 11
"#,
        );
        let resolver = TestResolver { root: root.clone() };
        let output_dir = root.join("bundle");

        let err =
            run_plan_step_stl_with_mode(&supported_sample_plan(), &output_dir, &resolver, true)
                .expect_err("supported runner-safe plan must hard fail");
        let diagnostic = format!("{} {}", err, err.details.as_deref().unwrap_or(""));
        assert!(
            diagnostic.contains("runner support gate accepted")
                || diagnostic.contains("forced unsupported"),
            "unexpected error: {err:?}"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn serializes_plan_json_for_runner_abi() {
        let plan_json = serialize_runner_plan(&shell_clause_plan())
            .expect("plan serialization")
            .expect("runner plan");
        let json: serde_json::Value = serde_json::from_str(&plan_json).expect("json");

        assert_eq!(json["schemaVersion"], 1);
        assert!(json["planId"].as_str().unwrap().starts_with("sha256:"));
        assert!(json.get("parameters").is_none());
        assert_eq!(json["parts"][0]["key"], "body");
        assert_eq!(json["parts"][0]["label"], "Body");
        assert_eq!(json["parts"][0]["root"], 2);
        assert_eq!(json["parts"][0]["commands"][1]["op"], "shell");
        assert_eq!(json["parts"][0]["commands"][0]["args"][0]["kind"], "number");
        assert_eq!(json["parts"][0]["commands"][1]["args"][1]["kind"], "ref");
        assert_eq!(
            json["parts"][0]["commands"][1]["keywords"][0]["kind"],
            "selector"
        );
        assert_eq!(
            json["parts"][0]["commands"][1]["keywords"][0]["value"]["kind"],
            "text"
        );
        assert_eq!(
            json["parts"][0]["commands"][1]["keywords"][0]["payload"]["kind"],
            "face"
        );
        assert_eq!(
            json["parts"][0]["commands"][1]["keywords"][0]["payload"]["type"],
            "clauses"
        );
    }

    #[test]
    fn serializes_edge_all_selector_without_runner_keyword() {
        let plan_json = serialize_runner_plan(&edge_all_fillet_plan())
            .expect("plan serialization")
            .expect("runner plan");
        let json: serde_json::Value = serde_json::from_str(&plan_json).expect("json");
        assert_eq!(
            json["parts"][0]["commands"][1]["keywords"]
                .as_array()
                .expect("keywords")
                .len(),
            0
        );
    }

    #[test]
    fn rejects_unresolved_params_during_runner_serialization() {
        let err = serialize_runner_plan(&sample_plan()).expect_err("unresolved param should fail");
        assert!(
            err.message.contains("requires resolved args"),
            "unexpected error: {:?}",
            err
        );
    }

    #[test]
    fn skips_unsupported_resolved_selector_plan_during_runner_serialization() {
        let plan = serialize_runner_plan(&unsupported_resolved_selector_plan())
            .expect("serialization should not error");
        assert!(
            plan.is_none(),
            "unsupported plan should skip runner serialization"
        );
    }

    #[test]
    fn discovers_runner_from_resources_and_skips_when_disabled() {
        let root = temp_root("discover");
        let runner = root
            .join("resources")
            .join("runtime")
            .join("occt")
            .join("bin")
            .join("direct-occt-runner");
        fs::create_dir_all(runner.parent().expect("runner parent")).expect("mkdir");
        fs::write(&runner, "#!/bin/sh\nexit 0\n").expect("write runner");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&runner).expect("metadata").permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&runner, permissions).expect("chmod");
        }
        let resolver = TestResolver { root };

        assert_eq!(
            discover_direct_occt_runner_with_mode(&resolver, true),
            Some(runner.clone())
        );
        assert_eq!(
            discover_direct_occt_runner_with_mode(&resolver, false),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn runner_failure_preserves_stdout_stderr_and_exit_status() {
        static LOCK: OnceLock<std::sync::Mutex<()>> = OnceLock::new();
        let _guard = LOCK
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .expect("lock");

        let root = temp_root("runner-failure");
        let runner = root
            .join("resources")
            .join("bin")
            .join("direct-occt-runner");
        fs::create_dir_all(runner.parent().expect("runner parent")).expect("mkdir");
        write_executable(
            &runner,
            r#"#!/bin/sh
echo "runner stdout" 
echo "runner stderr" >&2
exit 7
"#,
        );
        let resolver = TestResolver { root: root.clone() };
        let output_dir = root.join("bundle");
        let err =
            run_plan_step_stl_with_mode(&supported_sample_plan(), &output_dir, &resolver, true)
                .expect_err("runner failure");

        let details = err.details.expect("details");
        assert!(details.contains("runner stdout"));
        assert!(details.contains("runner stderr"));
        assert!(details.contains("exit: 7"));
        assert!(output_dir.join(PLAN_FILE_NAME).is_file());
    }

    #[cfg(unix)]
    #[test]
    fn keyword_runner_plan_skips_runner_for_structured_unsupported() {
        let root = temp_root("unsupported-json-skip");
        let runner = root
            .join("resources")
            .join("bin")
            .join("direct-occt-runner");
        let output_dir = root.join("bundle");
        fs::create_dir_all(runner.parent().expect("runner parent")).expect("mkdir");
        write_executable(
            &runner,
            r#"#!/bin/sh
echo '{"class":"validation_error","code":"unsupported_op","message":"unsupported direct OCCT op `box`","details":"unsupported direct OCCT op `box`"}' >&2
exit 3
"#,
        );
        let resolver = TestResolver { root: root.clone() };

        let outcome =
            run_plan_step_stl_with_mode(&supported_sample_plan(), &output_dir, &resolver, true)
                .expect("structured unsupported skip");

        assert!(outcome.is_none());
        assert!(output_dir.join(PLAN_FILE_NAME).is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn runner_first_path_uses_discovered_runner_and_writes_plan_and_artifacts() {
        static LOCK: OnceLock<std::sync::Mutex<()>> = OnceLock::new();
        let _guard = LOCK
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .expect("lock");

        let root = temp_root("runner-first-route");
        let source_dir = root.join("source");
        let runner = root
            .join("resources")
            .join("bin")
            .join("direct-occt-runner");
        let output_dir = root.join("bundle");
        fs::create_dir_all(&source_dir).expect("source dir");
        fs::create_dir_all(runner.parent().expect("runner parent")).expect("mkdir");
        fs::write(source_dir.join(MODEL_STEP_FILE_NAME), b"baseline-step").expect("step");
        fs::write(source_dir.join(PREVIEW_STL_FILE_NAME), b"baseline-stl").expect("stl");
        fs::write(
            source_dir.join("topology.json"),
            r#"{"parts":[{"partId":"body","label":"Body","edges":[],"faces":[]}]}"#,
        )
        .expect("topology");
        let runner_script = format!(
            r#"#!/bin/sh
set -eu
source_dir='{}'
plan=""
out=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --plan)
      plan=$2
      shift 2
      ;;
    --out)
      out=$2
      shift 2
      ;;
    *)
      echo "unexpected arg: $1" >&2
      exit 1
      ;;
  esac
done
mkdir -p "$out"
cp "$source_dir/model.step" "$out/model.step"
cp "$source_dir/preview.stl" "$out/preview.stl"
cp "$source_dir/topology.json" "$out/topology.json"
echo "fake runner plan: $plan"
 "#,
            source_dir.display()
        );
        write_executable(&runner, &runner_script);
        let resolver = TestResolver { root: root.clone() };

        let outcome =
            run_plan_step_stl_with_mode(&supported_sample_plan(), &output_dir, &resolver, true)
                .expect("runner export");
        let Some(crate::ecky_cad_host::direct_occt_sdk::NativeExportOutcome::Exported {
            step_path,
            stl_path,
        }) = outcome
        else {
            panic!("expected runner export");
        };

        let plan_json = fs::read_to_string(output_dir.join(PLAN_FILE_NAME)).expect("plan json");
        let plan: serde_json::Value = serde_json::from_str(&plan_json).expect("plan");
        assert_eq!(plan["schemaVersion"], 1);
        assert!(plan["planId"].as_str().unwrap().starts_with("sha256:"));
        assert_eq!(plan["parts"][0]["commands"][0]["op"], "box");

        assert_eq!(fs::read(&step_path).expect("step"), b"baseline-step");
        assert_eq!(fs::read(&stl_path).expect("stl"), b"baseline-stl");
        assert_eq!(
            fs::read_to_string(output_dir.join("topology.json")).expect("topology"),
            r#"{"parts":[{"partId":"body","label":"Body","edges":[],"faces":[]}]}"#
        );

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn keyword_free_frame_plan_uses_runner_when_available() {
        let root = temp_root("frame-op-runner");
        let runner = root
            .join("resources")
            .join("bin")
            .join("direct-occt-runner");
        let output_dir = root.join("bundle");
        fs::create_dir_all(runner.parent().expect("runner parent")).expect("mkdir");
        write_executable(
            &runner,
            r#"#!/bin/sh
out=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --out)
      out="$2"
      shift 2
      ;;
    --plan)
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done
mkdir -p "$out"
: > "$out/model.step"
: > "$out/preview.stl"
: > "$out/topology.json"
exit 0
"#,
        );
        let resolver = TestResolver { root: root.clone() };

        let outcome =
            run_plan_step_stl_with_mode(&keyword_free_plane_plan(), &output_dir, &resolver, true)
                .expect("frame runner export");

        let Some(crate::ecky_cad_host::direct_occt_sdk::NativeExportOutcome::Exported {
            step_path,
            stl_path,
        }) = outcome
        else {
            panic!("expected frame runner export");
        };

        assert!(output_dir.join(PLAN_FILE_NAME).is_file());
        assert!(output_dir.join("topology.json").is_file());
        assert!(step_path.is_file());
        assert!(stl_path.is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn supported_keyword_plans_use_runner_when_available() {
        let root = temp_root("keyword-op-runner");
        let runner = root
            .join("resources")
            .join("bin")
            .join("direct-occt-runner");
        let output_dir = root.join("bundle");
        fs::create_dir_all(runner.parent().expect("runner parent")).expect("mkdir");
        write_executable(
            &runner,
            r#"#!/bin/sh
out=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --out)
      out="$2"
      shift 2
      ;;
    --plan)
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done
mkdir -p "$out"
: > "$out/model.step"
: > "$out/preview.stl"
printf '{"parts":[{"partId":"body","label":"Body","edges":[],"faces":[]}]}' > "$out/topology.json"
exit 0
"#,
        );
        let resolver = TestResolver { root: root.clone() };

        for plan in [keyword_profile_holes_plan(), keyword_clip_box_plan()] {
            let outcome = run_plan_step_stl_with_mode(&plan, &output_dir, &resolver, true)
                .expect("keyword runner export");
            let Some(crate::ecky_cad_host::direct_occt_sdk::NativeExportOutcome::Exported {
                step_path,
                stl_path,
            }) = outcome
            else {
                panic!("expected keyword runner export");
            };

            assert!(output_dir.join(PLAN_FILE_NAME).is_file());
            assert!(output_dir.join("topology.json").is_file());
            assert!(step_path.is_file());
            assert!(stl_path.is_file());
        }

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn supported_exact_selector_plans_use_runner_when_available() {
        let root = temp_root("exact-selector-runner");
        let runner = root
            .join("resources")
            .join("bin")
            .join("direct-occt-runner");
        let output_dir = root.join("bundle");
        fs::create_dir_all(runner.parent().expect("runner parent")).expect("mkdir");
        write_executable(
            &runner,
            r#"#!/bin/sh
out=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --out)
      out="$2"
      shift 2
      ;;
    --plan)
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done
mkdir -p "$out"
: > "$out/model.step"
: > "$out/preview.stl"
printf '{"parts":[{"partId":"body","label":"Body","edges":[],"faces":[]}]}' > "$out/topology.json"
exit 0
"#,
        );
        let resolver = TestResolver { root: root.clone() };

        for plan in [
            shell_plan(),
            edge_all_fillet_plan(),
            edge_all_chamfer_plan(),
            exact_fillet_plan(),
            exact_chamfer_plan(),
            clause_fillet_plan(),
            clause_chamfer_plan(),
            exact_shell_plan(),
            shell_clause_plan(),
        ] {
            let outcome = run_plan_step_stl_with_mode(&plan, &output_dir, &resolver, true)
                .expect("exact selector runner export");
            let Some(crate::ecky_cad_host::direct_occt_sdk::NativeExportOutcome::Exported {
                step_path,
                stl_path,
            }) = outcome
            else {
                panic!("expected exact selector runner export");
            };

            assert!(output_dir.join(PLAN_FILE_NAME).is_file());
            assert!(output_dir.join("topology.json").is_file());
            assert!(step_path.is_file());
            assert!(stl_path.is_file());
        }

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn unsupported_keyword_runner_plan_skips_runner_for_generated_source_fallback() {
        let root = temp_root("unsupported-skip");
        let runner = root
            .join("resources")
            .join("bin")
            .join("direct-occt-runner");
        let output_dir = root.join("bundle");
        fs::create_dir_all(runner.parent().expect("runner parent")).expect("mkdir");
        write_executable(
            &runner,
            r#"#!/bin/sh
echo "runner should not run" >&2
exit 7
"#,
        );
        let resolver = TestResolver { root: root.clone() };

        let outcome = run_plan_step_stl_with_mode(
            &unsupported_resolved_selector_plan(),
            &output_dir,
            &resolver,
            true,
        )
        .expect("skip keyword runner plan");

        assert!(outcome.is_none());
        assert!(!output_dir.join(PLAN_FILE_NAME).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_precompiled_runner_exports_supported_plan_when_available() {
        let root = temp_root("live-precompiled");
        let resolver = TestResolver { root: root.clone() };
        let Some(runner) = discover_direct_occt_runner_with_mode(&resolver, true) else {
            return;
        };
        if !runner.is_file() {
            return;
        }

        let output_dir = root.join("bundle");
        let outcome =
            run_plan_step_stl_with_mode(&supported_sample_plan(), &output_dir, &resolver, true)
                .expect("live runner export");
        let Some(crate::ecky_cad_host::direct_occt_sdk::NativeExportOutcome::Exported {
            step_path,
            stl_path,
        }) = outcome
        else {
            panic!("expected live runner export");
        };

        assert!(output_dir.join(PLAN_FILE_NAME).is_file());
        assert!(output_dir.join("topology.json").is_file());
        assert!(
            fs::metadata(&step_path).expect("step metadata").len() > 1024,
            "STEP export too small"
        );
        assert!(
            fs::metadata(&stl_path).expect("stl metadata").len() > 512,
            "STL export too small"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_precompiled_runner_exports_expanded_keyword_free_subset_when_available() {
        let root = temp_root("live-precompiled-expanded");
        let resolver = TestResolver { root: root.clone() };
        let Some(runner) = discover_direct_occt_runner_with_mode(&resolver, true) else {
            return;
        };
        if !runner.is_file() {
            return;
        }

        let cases = [
            ("transform", expanded_transform_plan()),
            ("array", expanded_array_plan()),
            ("profile-surface", expanded_profile_surface_plan()),
            ("revolve", expanded_revolve_plan()),
            ("profile-offset-twist", expanded_profile_offset_twist_plan()),
        ];
        for (label, plan) in cases {
            assert!(
                runner_supports_plan(&plan),
                "runner support gate for {label}"
            );
            let output_dir = root.join(label);
            let outcome = run_plan_step_stl_with_mode(&plan, &output_dir, &resolver, true)
                .unwrap_or_else(|err| panic!("live runner export failed for {label}: {err}"));
            let Some(crate::ecky_cad_host::direct_occt_sdk::NativeExportOutcome::Exported {
                step_path,
                stl_path,
            }) = outcome
            else {
                panic!("expected live runner export for {label}");
            };

            assert!(
                output_dir.join(PLAN_FILE_NAME).is_file(),
                "missing plan for {label}"
            );
            assert!(
                output_dir.join("topology.json").is_file(),
                "missing topology for {label}"
            );
            assert!(
                fs::metadata(&step_path).expect("step metadata").len() > 1024,
                "STEP export too small for {label}"
            );
            assert!(
                fs::metadata(&stl_path).expect("stl metadata").len() > 512,
                "STL export too small for {label}"
            );
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_precompiled_runner_accepts_profile_holes_and_emits_target_ids_when_available() {
        let Some((root, topology)) =
            run_real_runner_plan_json("live-runner-profile-holes", &keyword_profile_holes_plan())
        else {
            return;
        };

        let edges = topology["parts"][0]["edges"].as_array().expect("edges");
        let faces = topology["parts"][0]["faces"].as_array().expect("faces");
        assert!(!edges.is_empty(), "expected edges");
        assert!(!faces.is_empty(), "expected faces");
        assert!(
            edges[0]["targetId"]
                .as_str()
                .expect("edge target id")
                .starts_with("body:edge:"),
            "unexpected edge target id: {}",
            edges[0]["targetId"]
        );
        assert!(
            faces[0]["targetId"]
                .as_str()
                .expect("face target id")
                .starts_with("body:face:"),
            "unexpected face target id: {}",
            faces[0]["targetId"]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_precompiled_runner_accepts_clip_box_keywords_when_available() {
        let Some((root, topology)) =
            run_real_runner_plan_json("live-runner-clip-box", &keyword_clip_box_plan())
        else {
            return;
        };

        let faces = topology["parts"][0]["faces"].as_array().expect("faces");
        assert!(!faces.is_empty(), "expected faces");
        assert!(
            faces[0]["targetId"]
                .as_str()
                .expect("face target id")
                .starts_with("body:face:"),
            "unexpected face target id: {}",
            faces[0]["targetId"]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_precompiled_runner_accepts_exact_selector_plans_when_available() {
        for (label, plan) in [
            ("live-runner-shell-default", shell_plan()),
            ("live-runner-fillet-edge-all", edge_all_fillet_plan()),
            ("live-runner-chamfer-edge-all", edge_all_chamfer_plan()),
            ("live-runner-fillet-exact", exact_fillet_plan()),
            ("live-runner-chamfer-exact", exact_chamfer_plan()),
            ("live-runner-fillet-clause", clause_fillet_plan()),
            ("live-runner-chamfer-clause", clause_chamfer_plan()),
            ("live-runner-shell-exact", exact_shell_plan()),
            ("live-runner-shell-clause", shell_clause_plan()),
        ] {
            let Some((root, topology)) = run_real_runner_plan_json(label, &plan) else {
                return;
            };

            let faces = topology["parts"][0]["faces"].as_array().expect("faces");
            assert!(!faces.is_empty(), "expected faces for {label}");
            let _ = fs::remove_dir_all(root);
        }
    }

    #[test]
    fn live_precompiled_runner_reports_structured_parse_and_schema_errors_when_available() {
        let Some((parse_root, parse_output)) =
            run_real_runner_plan_text("live-runner-parse-error", "{")
        else {
            return;
        };
        assert_eq!(parse_output.status.code(), Some(1));
        let parse_error: serde_json::Value =
            serde_json::from_slice(&parse_output.stderr).expect("parse error json");
        assert_eq!(parse_error["class"], "parse_error");
        assert_eq!(parse_error["code"], "parse_failed");
        let _ = fs::remove_dir_all(parse_root);

        let Some((schema_root, schema_output)) = run_real_runner_plan_text(
            "live-runner-schema-error",
            r#"{"schemaVersion":99,"planId":"bad","parts":[]}"#,
        ) else {
            return;
        };
        assert_eq!(schema_output.status.code(), Some(2));
        let schema_error: serde_json::Value =
            serde_json::from_slice(&schema_output.stderr).expect("schema error json");
        assert_eq!(schema_error["class"], "schema_error");
        assert_eq!(schema_error["code"], "schema_mismatch");
        let _ = fs::remove_dir_all(schema_root);

        let Some((param_root, param_output)) = run_real_runner_plan_text(
            "live-runner-param-schema-error",
            r#"{"schemaVersion":1,"planId":"bad","parts":[{"key":"body","label":"Body","root":1,"commands":[{"output":1,"op":"box","args":[{"kind":"param","value":"width"},{"kind":"number","value":8},{"kind":"number","value":4}],"keywords":[]}]}]}"#,
        ) else {
            return;
        };
        assert_eq!(param_output.status.code(), Some(2));
        let param_error: serde_json::Value =
            serde_json::from_slice(&param_output.stderr).expect("param schema error json");
        assert_eq!(param_error["class"], "schema_error");
        assert_eq!(param_error["code"], "schema_mismatch");
        let _ = fs::remove_dir_all(param_root);
    }
}
