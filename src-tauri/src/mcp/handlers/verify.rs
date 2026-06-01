use super::artifact_bundle_digest;
use crate::mcp::contracts::{StructuralVerificationSummaryResponse, VerifyGeneratedModelResponse};
use crate::models::{AppResult, AppState, ArtifactBundle, ModelManifest, PathResolver};
use std::collections::BTreeMap;

pub fn handle_verify_generated_model(
    state: &AppState,
    app: &dyn PathResolver,
    thread_id: &str,
    message_id: &str,
    model_id: &str,
    _original_prompt: &str,
) -> AppResult<VerifyGeneratedModelResponse> {
    let bundle = crate::model_runtime::read_artifact_bundle(app, model_id)?;
    let manifest = crate::model_runtime::read_model_manifest(app, model_id)?;
    let artifact_digest = artifact_bundle_digest(&bundle);
    let result = enrich_verify_result_with_diagnostic_context(
        crate::services::author_verification_foundation::verify_structure_with_author_verification(
            &bundle, &manifest,
        ),
        state,
        message_id,
        &bundle,
        &manifest,
    );
    Ok(VerifyGeneratedModelResponse {
        thread_id: thread_id.to_string(),
        message_id: message_id.to_string(),
        model_id: model_id.to_string(),
        artifact_digest,
        result,
    })
}

pub fn handle_structural_verification_summary(
    state: &AppState,
    app: &dyn PathResolver,
    thread_id: &str,
    message_id: &str,
    model_id: &str,
) -> AppResult<StructuralVerificationSummaryResponse> {
    let bundle = crate::model_runtime::read_artifact_bundle(app, model_id)?;
    let manifest = crate::model_runtime::read_model_manifest(app, model_id)?;
    let artifact_digest = artifact_bundle_digest(&bundle);
    let result = enrich_verify_result_with_diagnostic_context(
        crate::services::author_verification_foundation::verify_structure_with_author_verification(
            &bundle, &manifest,
        ),
        state,
        message_id,
        &bundle,
        &manifest,
    );
    Ok(StructuralVerificationSummaryResponse {
        thread_id: thread_id.to_string(),
        message_id: message_id.to_string(),
        model_id: model_id.to_string(),
        artifact_digest,
        passed: result.passed,
        summary: result.summary,
        issue_count: result.issues.len(),
        verifier_status: result.verifier_status,
        verifier_source: result.verifier_source,
    })
}

fn core_param_value_to_param_value(
    value: &crate::ecky_core_ir::CoreParameterValue,
) -> crate::models::ParamValue {
    match value {
        crate::ecky_core_ir::CoreParameterValue::Number(value) => {
            crate::models::ParamValue::Number(*value)
        }
        crate::ecky_core_ir::CoreParameterValue::Boolean(value) => {
            crate::models::ParamValue::Boolean(*value)
        }
        crate::ecky_core_ir::CoreParameterValue::Text(value)
        | crate::ecky_core_ir::CoreParameterValue::Choice(value)
        | crate::ecky_core_ir::CoreParameterValue::Image(value) => {
            crate::models::ParamValue::String(value.clone())
        }
    }
}

fn resolved_verify_diagnostic_params(
    state: &AppState,
    message_id: &str,
    bundle: &ArtifactBundle,
) -> Vec<crate::models::DiagnosticParamValue> {
    let mut resolved = BTreeMap::new();
    let Some(source_path) = bundle
        .macro_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
    else {
        return resolved
            .into_iter()
            .map(|(key, value)| crate::models::DiagnosticParamValue { key, value })
            .collect();
    };
    let Ok(source) = std::fs::read_to_string(source_path) else {
        return resolved
            .into_iter()
            .map(|(key, value)| crate::models::DiagnosticParamValue { key, value })
            .collect();
    };
    let Ok(program) = crate::ecky_scheme::compile_to_core_program(&source) else {
        return resolved
            .into_iter()
            .map(|(key, value)| crate::models::DiagnosticParamValue { key, value })
            .collect();
    };

    for param in &program.parameters {
        resolved.insert(
            param.key.clone(),
            core_param_value_to_param_value(&param.default_value),
        );
    }

    if let Ok(conn) = state.db.try_lock() {
        if let Ok(Some((output, _thread_id))) =
            crate::db::get_message_output_and_thread(&conn, message_id)
        {
            for (key, value) in output.initial_params {
                resolved.insert(key, value);
            }
        }
    }

    resolved
        .into_iter()
        .map(|(key, value)| crate::models::DiagnosticParamValue { key, value })
        .collect()
}

fn verify_check_op_name(check: &crate::models::AuthoredVerifyCheck) -> Option<String> {
    match (check.metric_source.as_deref(), check.metric_key.as_deref()) {
        (Some(source), Some(key)) => Some(format!("verify:{source}/{key}")),
        (Some(source), None) => Some(format!("verify:{source}")),
        (None, Some(key)) => Some(format!("verify:{key}")),
        (None, None) => Some(format!("verify:{}", check.tag)),
    }
}

fn enrich_verify_result_with_diagnostic_context(
    mut result: crate::models::StructuralVerificationResult,
    state: &AppState,
    message_id: &str,
    bundle: &ArtifactBundle,
    manifest: &ModelManifest,
) -> crate::models::StructuralVerificationResult {
    let part_key = (manifest.parts.len() == 1).then(|| manifest.parts[0].part_id.clone());
    let resolved_params = resolved_verify_diagnostic_params(state, message_id, bundle);

    let mut failing_contexts = Vec::new();
    for check in &mut result.authored_verify_checks {
        if check.status == crate::models::AuthoredVerifyCheckStatus::Passed {
            continue;
        }
        let context = crate::models::DiagnosticContext {
            part_key: part_key.clone(),
            op_name: verify_check_op_name(check),
            start_line: None,
            end_line: None,
            resolved_params: resolved_params.clone(),
        };
        check.diagnostic_context = Some(context.clone());
        failing_contexts.push(context);
    }

    let mut failing_index = 0usize;
    for issue in &mut result.issues {
        if !matches!(
            issue.code.as_str(),
            "AUTHORED_VERIFY_FAILED" | "AUTHORED_VERIFY_ERROR"
        ) {
            continue;
        }
        let Some(context) = failing_contexts.get(failing_index).cloned() else {
            break;
        };
        if issue.part_id.is_none() {
            issue.part_id = context.part_key.clone();
        }
        issue.diagnostic_context = Some(context);
        failing_index += 1;
    }

    result
}
