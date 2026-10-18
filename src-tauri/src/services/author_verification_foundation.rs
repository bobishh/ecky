use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    fs,
    path::{Path, PathBuf},
};

use crate::contracts::{
    ArtifactBundle, AuthoredVerifyCheck,
    AuthoredVerifyCheckStatus as PublicAuthorVerifyCheckStatus, AuthoredVerifySeverity,
    AuthoredVerifyValue, DesignParams, ManifestBounds, ModelManifest, ParamValue, StructuralIssue,
    StructuralVerificationResult,
};
use crate::ecky_core_ir::{CoreVerifyClause, CoreVerifyValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CoverageStats {
    pub present: usize,
    pub total: usize,
}

impl CoverageStats {
    #[cfg(test)]
    pub(crate) fn ratio(self) -> Option<f64> {
        (self.total > 0).then_some(self.present as f64 / self.total as f64)
    }

    #[cfg(test)]
    pub(crate) fn is_complete(self) -> bool {
        self.total > 0 && self.present == self.total
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ManifestAuthorMetrics {
    pub part_count: usize,
    pub editable_part_count: usize,
    pub parameter_group_count: usize,
    pub selection_target_count: usize,
    pub measurement_annotation_count: usize,
    pub warning_count: usize,
    pub bounds_coverage: CoverageStats,
    pub volume_coverage: CoverageStats,
    pub area_coverage: CoverageStats,
    pub viewer_asset_coverage: CoverageStats,
    pub total_volume_mm3: Option<f64>,
    pub total_area_mm2: Option<f64>,
    pub assembly_bounds: Option<ManifestBounds>,
}

impl ManifestAuthorMetrics {
    #[cfg(test)]
    pub(crate) fn geometric_coverage_complete(&self) -> bool {
        self.part_count > 0
            && self.bounds_coverage.is_complete()
            && self.volume_coverage.is_complete()
            && self.area_coverage.is_complete()
    }

    #[cfg(test)]
    pub(crate) fn viewer_asset_coverage_complete(&self) -> bool {
        self.viewer_asset_coverage.is_complete()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StructuralAuthorMetrics {
    pub passed: bool,
    pub issue_count: usize,
    pub issue_codes: BTreeMap<String, usize>,
    pub model_stl_size_bytes: Option<u64>,
    pub model_stl_triangle_count: Option<u32>,
    pub model_stl_component_count: Option<u32>,
    pub model_stl_non_manifold_edge_count: Option<u32>,
    pub model_stl_overhang_triangle_count: Option<u32>,
    pub model_stl_overhang_ratio: Option<f64>,
    pub total_volume_mm3: Option<f64>,
    pub total_area_mm2: Option<f64>,
    pub bbox: Option<ManifestBounds>,
}

impl StructuralAuthorMetrics {
    #[cfg(test)]
    pub(crate) fn is_passing(&self) -> bool {
        self.passed && self.issue_count == 0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AuthorVerificationMetrics {
    pub manifest: ManifestAuthorMetrics,
    pub structural: Option<StructuralAuthorMetrics>,
}

impl AuthorVerificationMetrics {
    #[cfg(test)]
    pub(crate) fn ready_for_author_verification(&self) -> bool {
        self.manifest.geometric_coverage_complete()
            && self
                .structural
                .as_ref()
                .is_some_and(StructuralAuthorMetrics::is_passing)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum AuthorVerifyResolvedValue {
    Number(f64),
    Boolean(bool),
    Text(String),
}

#[derive(Debug, Clone, Copy)]
struct ParsedVerifyMetricRef<'a> {
    source: &'a str,
    key: &'a str,
    args: &'a [CoreVerifyValue],
}

#[derive(Debug, Clone)]
enum DistanceAnchor {
    Point { point: [f64; 3] },
    Face { point: [f64; 3], normal: [f64; 3] },
    Segment { start: [f64; 3], end: [f64; 3] },
    Bounds { bounds: ManifestBounds },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthorVerifyCheckStatus {
    Passed,
    Failed,
    Error,
    Skipped,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AuthorVerifyCheckResult {
    pub clause_index: usize,
    pub status: AuthorVerifyCheckStatus,
    pub severity: AuthoredVerifySeverity,
    pub intent: Option<String>,
    pub condition: Option<String>,
    pub condition_result: Option<bool>,
    pub skip_reason: Option<String>,
    pub metric_alias: Option<String>,
    pub metric_source: Option<String>,
    pub metric_key: Option<String>,
    pub comparator: Option<String>,
    pub expected: Option<AuthorVerifyResolvedValue>,
    pub actual: Option<AuthorVerifyResolvedValue>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AuthorVerifyEvaluation {
    pub passed: bool,
    pub summary: String,
    pub checks: Vec<AuthorVerifyCheckResult>,
}

pub(crate) fn collect_author_verification_metrics(
    manifest: &ModelManifest,
    structural: Option<&StructuralVerificationResult>,
) -> AuthorVerificationMetrics {
    AuthorVerificationMetrics {
        manifest: collect_manifest_author_metrics(manifest),
        structural: structural.map(collect_structural_author_metrics),
    }
}

/// Canonical four-clause proof carried by printable thread authoring helpers.
/// `flank_degrees` is measured from horizontal; its FDM overhang is therefore
/// the complementary angle from vertical.
pub(crate) fn printed_thread_verify_clauses(
    pitch: f64,
    base_width: f64,
    flank_degrees: f64,
    max_overhang_degrees: f64,
) -> Vec<CoreVerifyClause> {
    fn clause(
        tag: &str,
        alias: &str,
        metric: CoreVerifyValue,
        operator: &str,
        expected: f64,
    ) -> CoreVerifyClause {
        CoreVerifyClause {
            tag: crate::ecky_core_ir::CoreVerifySection {
                items: vec![CoreVerifyValue::Symbol(tag.to_string())],
            },
            intent: None,
            severity: None,
            when: None,
            metric: crate::ecky_core_ir::CoreVerifySection {
                items: vec![CoreVerifyValue::Symbol(alias.to_string()), metric],
            },
            expect: crate::ecky_core_ir::CoreVerifySection {
                items: vec![
                    CoreVerifyValue::Symbol(alias.to_string()),
                    CoreVerifyValue::List(vec![
                        CoreVerifyValue::Symbol(operator.to_string()),
                        CoreVerifyValue::Number(expected),
                    ]),
                ],
            },
        }
    }

    let metric = |source: &str, key: &str, args: Vec<CoreVerifyValue>| {
        let mut items = vec![
            CoreVerifyValue::Symbol(source.to_string()),
            CoreVerifyValue::Symbol(key.to_string()),
        ];
        items.extend(args);
        CoreVerifyValue::List(items)
    };

    vec![
        clause(
            "printed_thread_single_solid",
            "components",
            metric("stl", "connected-component-count", vec![]),
            "=",
            1.0,
        ),
        clause(
            "printed_thread_manifold",
            "bad_edges",
            metric("stl", "non-manifold-edge-count", vec![]),
            "=",
            0.0,
        ),
        clause(
            "printed_thread_overhang",
            "overhang",
            metric(
                "thread",
                "overhang-degrees",
                vec![CoreVerifyValue::Number(flank_degrees)],
            ),
            "<=",
            max_overhang_degrees,
        ),
        clause(
            "printed_thread_turn_clearance",
            "pitch_margin",
            metric(
                "thread",
                "pitch-base-margin",
                vec![
                    CoreVerifyValue::Number(pitch),
                    CoreVerifyValue::Number(base_width),
                ],
            ),
            ">",
            0.0,
        ),
    ]
}

fn expand_printed_thread_verify_sets(
    clauses: &[CoreVerifyClause],
) -> Result<Vec<CoreVerifyClause>, String> {
    let mut expanded = Vec::new();
    for clause in clauses {
        let Some(metric_value) = clause.metric.items.get(1) else {
            expanded.push(clause.clone());
            continue;
        };
        let metric = parse_metric_ref(metric_value)?;
        if metric.source != "thread" || metric.key != "printed-thread-set" {
            expanded.push(clause.clone());
            continue;
        }
        let values = metric
            .args
            .iter()
            .map(|value| match value {
                CoreVerifyValue::Number(number) if number.is_finite() => Ok(*number),
                _ => Err(
                    "`(thread printed-thread-set ...)` expects four finite numbers: pitch, base width, flank degrees, max overhang degrees."
                        .to_string(),
                ),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let [pitch, base_width, flank_degrees, max_overhang_degrees] = values.as_slice() else {
            return Err(
                "`(thread printed-thread-set ...)` expects four numbers: pitch, base width, flank degrees, max overhang degrees."
                    .to_string(),
            );
        };
        expanded.extend(printed_thread_verify_clauses(
            *pitch,
            *base_width,
            *flank_degrees,
            *max_overhang_degrees,
        ));
    }
    Ok(expanded)
}

#[cfg(test)]
pub(crate) fn evaluate_author_verify_clauses(
    clauses: &[CoreVerifyClause],
    bundle: &ArtifactBundle,
    manifest: &ModelManifest,
    structural: Option<&StructuralVerificationResult>,
) -> AuthorVerifyEvaluation {
    let params = default_verify_params(bundle);
    evaluate_author_verify_clauses_with_params(clauses, bundle, manifest, structural, &params)
}

pub(crate) fn evaluate_author_verify_clauses_with_params(
    clauses: &[CoreVerifyClause],
    bundle: &ArtifactBundle,
    manifest: &ModelManifest,
    structural: Option<&StructuralVerificationResult>,
    effective_params: &DesignParams,
) -> AuthorVerifyEvaluation {
    if clauses.is_empty() {
        return AuthorVerifyEvaluation {
            passed: true,
            summary: "No authored verify clauses.".to_string(),
            checks: Vec::new(),
        };
    }

    let metrics = collect_author_verification_metrics(manifest, structural);
    let checks = clauses
        .iter()
        .enumerate()
        .map(|(index, clause)| {
            evaluate_verify_clause(index, clause, bundle, manifest, &metrics, effective_params)
        })
        .collect::<Vec<_>>();
    let failed_errors = checks
        .iter()
        .filter(|check| {
            check.status == AuthorVerifyCheckStatus::Failed
                && check.severity == AuthoredVerifySeverity::Error
        })
        .count();
    let failed_warnings = checks
        .iter()
        .filter(|check| {
            check.status == AuthorVerifyCheckStatus::Failed
                && check.severity == AuthoredVerifySeverity::Warning
        })
        .count();
    let errored = checks
        .iter()
        .filter(|check| check.status == AuthorVerifyCheckStatus::Error)
        .count();
    let skipped = checks
        .iter()
        .filter(|check| check.status == AuthorVerifyCheckStatus::Skipped)
        .count();

    let summary = if failed_errors == 0 && errored == 0 {
        format!(
            "Authored verify passed with {failed_warnings} warning(s) and {skipped} skipped check(s)."
        )
    } else if failed_errors > 0 && errored == 0 {
        format!("{failed_errors} authored verify error-severity check(s) failed.")
    } else if failed_errors == 0 {
        format!("{errored} authored verify check(s) errored.")
    } else {
        format!("{failed_errors} authored verify check(s) failed; {errored} errored.")
    };

    AuthorVerifyEvaluation {
        passed: failed_errors == 0 && errored == 0,
        summary,
        checks,
    }
}

pub(crate) fn verify_structure_with_author_verification(
    bundle: &ArtifactBundle,
    manifest: &ModelManifest,
) -> StructuralVerificationResult {
    let params = default_verify_params(bundle);
    verify_structure_with_author_verification_and_params(bundle, manifest, &params)
}

pub(crate) fn verify_structure_with_author_verification_and_params(
    bundle: &ArtifactBundle,
    manifest: &ModelManifest,
    effective_params: &DesignParams,
) -> StructuralVerificationResult {
    match load_authored_verify_clauses(bundle) {
        LoadedAuthoredVerifyClauses::Unavailable => {
            crate::services::structural_verification::verify_structure(bundle, manifest)
        }
        LoadedAuthoredVerifyClauses::Error(message) => {
            let mut result =
                crate::services::structural_verification::verify_structure(bundle, manifest);
            push_authored_verify_error(&mut result, message);
            finalize_structural_verification_result(result)
        }
        LoadedAuthoredVerifyClauses::Loaded(verify_clauses) => {
            let expected_component_count = exact_stl_component_count_contract(&verify_clauses);
            let result = crate::services::structural_verification::verify_structure_with_expected_component_count(
                bundle,
                manifest,
                expected_component_count,
            );
            merge_loaded_author_verification_into_structural_result(
                bundle,
                manifest,
                result,
                &verify_clauses,
                effective_params,
            )
        }
    }
}

#[cfg(test)]
pub(crate) fn merge_author_verification_into_structural_result(
    bundle: &ArtifactBundle,
    manifest: &ModelManifest,
    result: StructuralVerificationResult,
) -> StructuralVerificationResult {
    match load_authored_verify_clauses(bundle) {
        LoadedAuthoredVerifyClauses::Unavailable => result,
        LoadedAuthoredVerifyClauses::Error(message) => {
            let mut result = result;
            push_authored_verify_error(&mut result, message);
            finalize_structural_verification_result(result)
        }
        LoadedAuthoredVerifyClauses::Loaded(verify_clauses) => {
            merge_loaded_author_verification_into_structural_result(
                bundle,
                manifest,
                result,
                &verify_clauses,
                &default_verify_params(bundle),
            )
        }
    }
}

enum LoadedAuthoredVerifyClauses {
    Unavailable,
    Loaded(Vec<CoreVerifyClause>),
    Error(String),
}

fn load_authored_verify_clauses(bundle: &ArtifactBundle) -> LoadedAuthoredVerifyClauses {
    if bundle.source_language != crate::contracts::SourceLanguage::EckyIrV0 {
        return LoadedAuthoredVerifyClauses::Unavailable;
    }
    let Some(source_path) = bundle
        .macro_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
    else {
        return LoadedAuthoredVerifyClauses::Unavailable;
    };
    let Ok(source) = fs::read_to_string(source_path) else {
        return LoadedAuthoredVerifyClauses::Unavailable;
    };
    let program = match crate::ecky_scheme::compile_to_core_program(&source) {
        Ok(program) => program,
        Err(err) => {
            return LoadedAuthoredVerifyClauses::Error(format!(
                "Authored verify source could not compile for evaluation: {}",
                err
            ));
        }
    };
    if program.constraints.verify_clauses.is_empty() {
        return LoadedAuthoredVerifyClauses::Unavailable;
    }
    match expand_printed_thread_verify_sets(&program.constraints.verify_clauses) {
        Ok(clauses) => LoadedAuthoredVerifyClauses::Loaded(clauses),
        Err(message) => LoadedAuthoredVerifyClauses::Error(message),
    }
}

fn default_verify_params(bundle: &ArtifactBundle) -> DesignParams {
    let Some(source_path) = bundle
        .macro_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
    else {
        return DesignParams::new();
    };
    let Ok(source) = fs::read_to_string(source_path) else {
        return DesignParams::new();
    };
    let Ok(program) = crate::ecky_scheme::compile_to_core_program(&source) else {
        return DesignParams::new();
    };
    program
        .parameters
        .into_iter()
        .map(|parameter| {
            let value = match parameter.default_value {
                crate::ecky_core_ir::CoreParameterValue::Number(value) => ParamValue::Number(value),
                crate::ecky_core_ir::CoreParameterValue::Boolean(value) => {
                    ParamValue::Boolean(value)
                }
                crate::ecky_core_ir::CoreParameterValue::Text(value)
                | crate::ecky_core_ir::CoreParameterValue::Choice(value)
                | crate::ecky_core_ir::CoreParameterValue::Image(value) => {
                    ParamValue::String(value)
                }
            };
            (parameter.key, value)
        })
        .collect()
}

fn exact_stl_component_count_contract(clauses: &[CoreVerifyClause]) -> Option<usize> {
    let mut declared = None;
    for clause in clauses {
        if clause.when.is_some()
            || matches!(
                verify_severity(clause),
                Ok(AuthoredVerifySeverity::Warning) | Err(_)
            )
        {
            continue;
        }
        let metric = parse_metric_ref(clause.metric.items.get(1)?).ok()?;
        if metric.source != "stl" || metric.key != "connected-component-count" {
            continue;
        }
        let (operator, expected) = parse_expected_comparison(clause.expect.items.get(1)?).ok()?;
        let AuthorVerifyResolvedValue::Number(expected) = expected else {
            return None;
        };
        if operator != "="
            || !expected.is_finite()
            || expected < 1.0
            || expected.fract() != 0.0
            || expected > usize::MAX as f64
        {
            return None;
        }
        let expected = expected as usize;
        match declared {
            Some(previous) if previous != expected => return None,
            Some(_) => {}
            None => declared = Some(expected),
        }
    }
    declared
}

fn push_authored_verify_error(result: &mut StructuralVerificationResult, message: String) {
    result.issues.push(StructuralIssue {
        code: "AUTHORED_VERIFY_ERROR".to_string(),
        message,
        part_id: None,
        numeric_payload: None,
        diagnostic_context: None,
    });
}

fn merge_loaded_author_verification_into_structural_result(
    bundle: &ArtifactBundle,
    manifest: &ModelManifest,
    mut result: StructuralVerificationResult,
    verify_clauses: &[CoreVerifyClause],
    effective_params: &DesignParams,
) -> StructuralVerificationResult {
    let evaluation = evaluate_author_verify_clauses_with_params(
        verify_clauses,
        bundle,
        manifest,
        Some(&result),
        effective_params,
    );
    result.authored_verify_checks = evaluation
        .checks
        .iter()
        .map(|check| {
            let clause = verify_clauses.get(check.clause_index);
            authored_verify_check_contract(check, clause)
        })
        .collect();
    if evaluation.passed {
        return result;
    }

    for check in evaluation.checks {
        if matches!(
            check.status,
            AuthorVerifyCheckStatus::Passed | AuthorVerifyCheckStatus::Skipped
        ) || (check.status == AuthorVerifyCheckStatus::Failed
            && check.severity == AuthoredVerifySeverity::Warning)
        {
            continue;
        }
        result.issues.push(StructuralIssue {
            code: match check.status {
                AuthorVerifyCheckStatus::Passed => "AUTHORED_VERIFY_PASSED".to_string(),
                AuthorVerifyCheckStatus::Failed => "AUTHORED_VERIFY_FAILED".to_string(),
                AuthorVerifyCheckStatus::Error => "AUTHORED_VERIFY_ERROR".to_string(),
                AuthorVerifyCheckStatus::Skipped => "AUTHORED_VERIFY_SKIPPED".to_string(),
            },
            message: check.message,
            part_id: None,
            numeric_payload: match check.actual {
                Some(AuthorVerifyResolvedValue::Number(value)) => Some(value),
                _ => None,
            },
            diagnostic_context: None,
        });
    }

    finalize_structural_verification_result(result)
}

fn authored_verify_check_contract(
    check: &AuthorVerifyCheckResult,
    clause: Option<&CoreVerifyClause>,
) -> AuthoredVerifyCheck {
    let tag = clause
        .map(authored_verify_tag)
        .filter(|tag| !tag.is_empty())
        .unwrap_or_else(|| format!("verify-{}", check.clause_index + 1));
    AuthoredVerifyCheck {
        // The New Params macro map keys verify nodes `verify:<tag>`; using the
        // same id makes the verify chip clickable -> focuses that node.
        stable_node_id: Some(format!("verify:{tag}")),
        status: match check.status {
            AuthorVerifyCheckStatus::Passed => PublicAuthorVerifyCheckStatus::Passed,
            AuthorVerifyCheckStatus::Failed => PublicAuthorVerifyCheckStatus::Failed,
            AuthorVerifyCheckStatus::Error => PublicAuthorVerifyCheckStatus::Error,
            AuthorVerifyCheckStatus::Skipped => PublicAuthorVerifyCheckStatus::Skipped,
        },
        severity: check.severity,
        tag,
        message: check.message.clone(),
        intent: check.intent.clone(),
        condition: check.condition.clone(),
        condition_result: check.condition_result,
        skip_reason: check.skip_reason.clone(),
        metric_source: check.metric_source.clone(),
        metric_key: check.metric_key.clone(),
        comparator: check.comparator.clone(),
        expected: check.expected.as_ref().map(authored_verify_value_contract),
        actual: check.actual.as_ref().map(authored_verify_value_contract),
        diagnostic_context: None,
    }
}

fn authored_verify_value_contract(value: &AuthorVerifyResolvedValue) -> AuthoredVerifyValue {
    match value {
        AuthorVerifyResolvedValue::Number(number) => AuthoredVerifyValue::Number(*number),
        AuthorVerifyResolvedValue::Boolean(flag) => AuthoredVerifyValue::Boolean(*flag),
        AuthorVerifyResolvedValue::Text(text) => AuthoredVerifyValue::Text(text.clone()),
    }
}

fn authored_verify_tag(clause: &CoreVerifyClause) -> String {
    clause
        .tag
        .items
        .iter()
        .filter_map(verify_symbol_like)
        .collect::<Vec<_>>()
        .join("/")
}

fn finalize_structural_verification_result(
    mut result: StructuralVerificationResult,
) -> StructuralVerificationResult {
    result.passed = result.issues.is_empty();
    result.summary = if result.passed {
        "All structural checks passed.".to_string()
    } else {
        let codes = result
            .issues
            .iter()
            .map(|issue| issue.code.as_str())
            .collect::<Vec<_>>();
        format!("Structural verification failed: {}", codes.join(", "))
    };
    result
}

fn collect_manifest_author_metrics(manifest: &ModelManifest) -> ManifestAuthorMetrics {
    let part_count = manifest.parts.len();
    let bounds_coverage = CoverageStats {
        present: manifest
            .parts
            .iter()
            .filter(|part| part.bounds.as_ref().is_some_and(bounds_are_finite))
            .count(),
        total: part_count,
    };
    let volume_coverage = CoverageStats {
        present: manifest
            .parts
            .iter()
            .filter(|part| part.volume.is_some_and(f64::is_finite))
            .count(),
        total: part_count,
    };
    let area_coverage = CoverageStats {
        present: manifest
            .parts
            .iter()
            .filter(|part| part.area.is_some_and(f64::is_finite))
            .count(),
        total: part_count,
    };
    let viewer_asset_coverage = CoverageStats {
        present: manifest
            .parts
            .iter()
            .filter(|part| {
                part.viewer_asset_path
                    .as_ref()
                    .is_some_and(|path| !path.trim().is_empty())
            })
            .count(),
        total: part_count,
    };

    ManifestAuthorMetrics {
        part_count,
        editable_part_count: manifest.parts.iter().filter(|part| part.editable).count(),
        parameter_group_count: manifest.parameter_groups.len(),
        selection_target_count: manifest.selection_targets.len(),
        measurement_annotation_count: manifest.measurement_annotations.len(),
        warning_count: manifest.warnings.len() + manifest.document.warnings.len(),
        bounds_coverage,
        volume_coverage,
        area_coverage,
        viewer_asset_coverage,
        total_volume_mm3: sum_finite_values(manifest.parts.iter().filter_map(|part| part.volume)),
        total_area_mm2: sum_finite_values(manifest.parts.iter().filter_map(|part| part.area)),
        assembly_bounds: union_bounds(
            manifest
                .parts
                .iter()
                .filter_map(|part| part.bounds.as_ref()),
        ),
    }
}

fn collect_structural_author_metrics(
    result: &StructuralVerificationResult,
) -> StructuralAuthorMetrics {
    let mut issue_codes = BTreeMap::new();
    for issue in &result.issues {
        *issue_codes.entry(issue.code.clone()).or_insert(0) += 1;
    }

    StructuralAuthorMetrics {
        passed: result.passed,
        issue_count: result.issues.len(),
        issue_codes,
        model_stl_size_bytes: result.metrics.model_stl_size_bytes,
        model_stl_triangle_count: result.metrics.model_stl_triangle_count,
        model_stl_component_count: result.metrics.model_stl_component_count,
        model_stl_non_manifold_edge_count: result.metrics.model_stl_non_manifold_edge_count,
        model_stl_overhang_triangle_count: result.metrics.model_stl_overhang_triangle_count,
        model_stl_overhang_ratio: result.metrics.model_stl_overhang_ratio,
        total_volume_mm3: result.metrics.total_volume,
        total_area_mm2: result.metrics.total_area,
        bbox: result.metrics.bbox.clone(),
    }
}

fn verify_intent(clause: &CoreVerifyClause) -> Result<Option<String>, String> {
    let Some(section) = &clause.intent else {
        return Ok(None);
    };
    match section.items.as_slice() {
        [CoreVerifyValue::Text(value) | CoreVerifyValue::Symbol(value)]
            if !value.trim().is_empty() =>
        {
            Ok(Some(value.clone()))
        }
        _ => Err("Verify intent expects exactly one non-empty text or symbol value.".to_string()),
    }
}

fn verify_severity(clause: &CoreVerifyClause) -> Result<AuthoredVerifySeverity, String> {
    let Some(section) = &clause.severity else {
        return Ok(AuthoredVerifySeverity::Error);
    };
    match section.items.as_slice() {
        [value] => match verify_symbol_like(value) {
            Some("error") => Ok(AuthoredVerifySeverity::Error),
            Some("warning") => Ok(AuthoredVerifySeverity::Warning),
            Some(other) => Err(format!(
                "Unsupported verify severity `{other}`; expected `error` or `warning`."
            )),
            None => Err("Verify severity expects `error` or `warning`.".to_string()),
        },
        _ => Err("Verify severity expects exactly one value.".to_string()),
    }
}

fn verify_condition(
    clause: &CoreVerifyClause,
    effective_params: &DesignParams,
) -> Result<(Option<String>, Option<bool>), String> {
    let Some(section) = &clause.when else {
        return Ok((None, None));
    };
    let [expression] = section.items.as_slice() else {
        return Err("Verify when expects exactly one boolean expression.".to_string());
    };
    Ok((
        Some(format_verify_value(expression)),
        Some(evaluate_verify_condition(expression, effective_params)?),
    ))
}

fn evaluate_verify_condition(
    value: &CoreVerifyValue,
    effective_params: &DesignParams,
) -> Result<bool, String> {
    match value {
        CoreVerifyValue::Boolean(value) => Ok(*value),
        CoreVerifyValue::Symbol(symbol) if symbol == "true" => Ok(true),
        CoreVerifyValue::Symbol(symbol) if symbol == "false" => Ok(false),
        CoreVerifyValue::Symbol(symbol) => match effective_params.get(symbol) {
            Some(ParamValue::Boolean(value)) => Ok(*value),
            Some(value) => Err(format!(
                "Verify when parameter `{symbol}` must be boolean, found {}.",
                value.kind()
            )),
            None => Err(format!(
                "Verify when references unknown parameter `{symbol}`."
            )),
        },
        CoreVerifyValue::List(items) => {
            let Some(operator) = items.first().and_then(verify_symbol_like) else {
                return Err("Verify when list needs a boolean operator.".to_string());
            };
            match (operator, &items[1..]) {
                ("not", [operand]) => Ok(!evaluate_verify_condition(operand, effective_params)?),
                ("not", _) => Err("Verify when `not` expects exactly one operand.".to_string()),
                ("and", []) => Err("Verify when `and` expects at least one operand.".to_string()),
                ("and", operands) => {
                    for operand in operands {
                        if !evaluate_verify_condition(operand, effective_params)? {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                }
                ("or", []) => Err("Verify when `or` expects at least one operand.".to_string()),
                ("or", operands) => {
                    for operand in operands {
                        if evaluate_verify_condition(operand, effective_params)? {
                            return Ok(true);
                        }
                    }
                    Ok(false)
                }
                (other, _) => Err(format!("Unsupported verify when operator `{other}`.")),
            }
        }
        _ => Err("Verify when operands must be booleans or boolean parameter names.".to_string()),
    }
}

fn format_verify_value(value: &CoreVerifyValue) -> String {
    match value {
        CoreVerifyValue::Symbol(value) => value.clone(),
        CoreVerifyValue::Number(value) => value.to_string(),
        CoreVerifyValue::Boolean(value) => if *value { "#t" } else { "#f" }.to_string(),
        CoreVerifyValue::Text(value) => format!("\"{}\"", value.replace('"', "\\\"")),
        CoreVerifyValue::List(items) => format!(
            "({})",
            items
                .iter()
                .map(format_verify_value)
                .collect::<Vec<_>>()
                .join(" ")
        ),
    }
}

fn evaluate_verify_clause(
    clause_index: usize,
    clause: &CoreVerifyClause,
    bundle: &ArtifactBundle,
    manifest: &ModelManifest,
    metrics: &AuthorVerificationMetrics,
    effective_params: &DesignParams,
) -> AuthorVerifyCheckResult {
    let intent = match verify_intent(clause) {
        Ok(intent) => intent,
        Err(message) => return verify_error(clause_index, None, &message),
    };
    let severity = match verify_severity(clause) {
        Ok(severity) => severity,
        Err(message) => return verify_error(clause_index, None, &message),
    };
    let (condition, condition_result) = match verify_condition(clause, effective_params) {
        Ok(result) => result,
        Err(message) => {
            let mut result = verify_error(clause_index, None, &message);
            result.intent = intent;
            result.severity = severity;
            return result;
        }
    };
    if condition_result == Some(false) {
        return AuthorVerifyCheckResult {
            clause_index,
            status: AuthorVerifyCheckStatus::Skipped,
            severity,
            intent,
            condition,
            condition_result,
            skip_reason: Some("Authored `when` condition resolved false.".to_string()),
            metric_alias: None,
            metric_source: None,
            metric_key: None,
            comparator: None,
            expected: None,
            actual: None,
            message: "Skipped because authored `when` condition resolved false.".to_string(),
        };
    }
    let alias = clause
        .metric
        .items
        .first()
        .and_then(verify_symbol_like)
        .map(str::to_string);

    let Some(metric_ref) = clause.metric.items.get(1) else {
        return verify_error(
            clause_index,
            alias,
            "Verify metric clause needs metric ref.",
        );
    };
    let Some(expect_ref) = clause.expect.items.first().and_then(verify_symbol_like) else {
        return verify_error(
            clause_index,
            alias,
            "Verify expect clause needs metric alias reference.",
        );
    };
    if alias.as_deref() != Some(expect_ref) {
        return verify_error(
            clause_index,
            alias,
            "Verify expect alias does not match metric alias.",
        );
    }
    let Some(expect_expr) = clause.expect.items.get(1) else {
        return verify_error(
            clause_index,
            alias,
            "Verify expect clause needs comparison expression.",
        );
    };

    let metric_ref = match parse_metric_ref(metric_ref) {
        Ok(parsed) => parsed,
        Err(message) => return verify_error(clause_index, alias, &message),
    };
    let expected = match parse_expected_comparison(expect_expr) {
        Ok(parsed) => parsed,
        Err(message) => return verify_error(clause_index, alias, &message),
    };
    let actual = match resolve_metric_value(
        metric_ref.source,
        metric_ref.key,
        metric_ref.args,
        bundle,
        manifest,
        metrics,
    ) {
        Ok(value) => value,
        Err(message) => {
            return AuthorVerifyCheckResult {
                clause_index,
                status: AuthorVerifyCheckStatus::Error,
                severity,
                intent,
                condition,
                condition_result,
                skip_reason: None,
                metric_alias: alias,
                metric_source: Some(metric_ref.source.to_string()),
                metric_key: Some(metric_ref.key.to_string()),
                comparator: Some(expected.0.to_string()),
                expected: Some(expected.1.clone()),
                actual: None,
                message,
            };
        }
    };

    let passed = compare_values(&actual, expected.0, &expected.1);
    AuthorVerifyCheckResult {
        clause_index,
        status: if passed {
            AuthorVerifyCheckStatus::Passed
        } else {
            AuthorVerifyCheckStatus::Failed
        },
        severity,
        intent,
        condition,
        condition_result,
        skip_reason: None,
        metric_alias: alias,
        metric_source: Some(metric_ref.source.to_string()),
        metric_key: Some(metric_ref.key.to_string()),
        comparator: Some(expected.0.to_string()),
        expected: Some(expected.1.clone()),
        actual: Some(actual.clone()),
        message: format!(
            "{} {} {}",
            describe_value(&actual),
            expected.0,
            describe_value(&expected.1)
        ),
    }
}

fn parse_metric_ref(value: &CoreVerifyValue) -> Result<ParsedVerifyMetricRef<'_>, String> {
    let CoreVerifyValue::List(items) = value else {
        return Err("Verify metric ref must be a list like `(manifest has-step)`.".to_string());
    };
    if items.len() < 2 {
        return Err("Verify metric ref must contain namespace and metric key.".to_string());
    }
    let source = verify_symbol_like(&items[0])
        .ok_or_else(|| "Verify metric namespace must be symbol or text.".to_string())?;
    let key = verify_symbol_like(&items[1])
        .ok_or_else(|| "Verify metric key must be symbol or text.".to_string())?;
    Ok(ParsedVerifyMetricRef {
        source,
        key,
        args: &items[2..],
    })
}

fn parse_expected_comparison(
    value: &CoreVerifyValue,
) -> Result<(&'static str, AuthorVerifyResolvedValue), String> {
    let CoreVerifyValue::List(items) = value else {
        return Err("Verify expect expression must be a list like `(> 3)`.".to_string());
    };
    let expected_value = match items.as_slice() {
        [_, literal] => literal,
        // Accept the documented `value` placeholder form as sugar for the
        // metric alias supplied by the surrounding `(expect alias ...)`.
        [_, placeholder, literal] if verify_symbol_like(placeholder) == Some("value") => literal,
        _ => return Err("Verify expect expression must contain operator and literal.".to_string()),
    };
    let op = verify_symbol_like(&items[0])
        .ok_or_else(|| "Verify comparison operator must be symbol or text.".to_string())?;
    let op = match op {
        "=" | "!=" | ">" | ">=" | "<" | "<=" => op,
        other => return Err(format!("Unsupported verify comparison operator `{other}`.")),
    };
    let expected = resolve_literal(expected_value)
        .ok_or_else(|| "Verify expect literal must be boolean, number, or text.".to_string())?;
    Ok((
        match op {
            "=" => "=",
            "!=" => "!=",
            ">" => ">",
            ">=" => ">=",
            "<" => "<",
            "<=" => "<=",
            _ => unreachable!(),
        },
        expected,
    ))
}

fn resolve_metric_value(
    source: &str,
    key: &str,
    args: &[CoreVerifyValue],
    bundle: &ArtifactBundle,
    manifest: &ModelManifest,
    metrics: &AuthorVerificationMetrics,
) -> Result<AuthorVerifyResolvedValue, String> {
    match source {
        "manifest" => {
            if !args.is_empty() {
                return Err("Manifest verify metrics do not accept selector arguments.".to_string());
            }
            resolve_manifest_metric_value(key, bundle, metrics)
        }
        "stl" => resolve_stl_metric_value(key, args, bundle, manifest, metrics),
        "clearance" => resolve_clearance_metric_value(key, args, bundle, manifest),
        "selector" => resolve_selector_metric_value(key, args, bundle, manifest),
        "relation" => resolve_relation_metric_value(key, args, bundle, manifest),
        "thread" => resolve_thread_metric_value(key, args, metrics),
        other => Err(format!("Unsupported verify metric namespace `{other}`.")),
    }
}

fn resolve_thread_metric_value(
    key: &str,
    args: &[CoreVerifyValue],
    metrics: &AuthorVerificationMetrics,
) -> Result<AuthorVerifyResolvedValue, String> {
    let number = |index: usize| {
        args.get(index)
            .and_then(|value| match value {
                CoreVerifyValue::Number(number) if number.is_finite() => Some(*number),
                _ => None,
            })
            .ok_or_else(|| {
                format!("Thread verify metric `{key}` expects finite numeric arguments.")
            })
    };
    match key {
        "overhang-degrees" if args.len() == 1 => Ok(AuthorVerifyResolvedValue::Number(
            (90.0 - number(0)?).max(0.0),
        )),
        "pitch-base-margin" if args.len() == 2 => {
            Ok(AuthorVerifyResolvedValue::Number(number(0)? - number(1)?))
        }
        "core-volume-ratio" if args.len() == 2 => {
            let radius = number(0)?;
            let length = number(1)?;
            if radius <= 0.0 || length <= 0.0 {
                return Err(
                    "Thread verify metric `core-volume-ratio` expects positive radius and length."
                        .to_string(),
                );
            }
            let actual_volume = metrics.manifest.total_volume_mm3.ok_or_else(|| {
                "Thread core-volume sanity requires manifest volume evidence.".to_string()
            })?;
            let expected_core_volume = std::f64::consts::PI * radius * radius * length;
            Ok(AuthorVerifyResolvedValue::Number(
                actual_volume / expected_core_volume,
            ))
        }
        "overhang-degrees" | "pitch-base-margin" | "core-volume-ratio" => Err(format!(
            "Thread verify metric `{key}` received the wrong argument count."
        )),
        other => Err(format!("Unsupported thread verify metric `{other}`.")),
    }
}

fn resolve_clearance_metric_value(
    key: &str,
    args: &[CoreVerifyValue],
    bundle: &ArtifactBundle,
    manifest: &ModelManifest,
) -> Result<AuthorVerifyResolvedValue, String> {
    match key {
        "min-distance" => {
            if args.len() != 2 {
                return Err(
                    "Clearance min-distance expects exactly two selector arguments.".to_string(),
                );
            }
            let left_selector = verify_symbol_like(&args[0])
                .ok_or_else(|| "Clearance selector A must be symbol or text.".to_string())?;
            let right_selector = verify_symbol_like(&args[1])
                .ok_or_else(|| "Clearance selector B must be symbol or text.".to_string())?;
            let left = resolve_distance_selector_anchors(left_selector, bundle, manifest)?;
            let right = resolve_distance_selector_anchors(right_selector, bundle, manifest)?;
            let distance = min_pairwise_distance(&left, &right);
            Ok(AuthorVerifyResolvedValue::Number(distance))
        }
        other => Err(format!("Unsupported clearance verify metric `{other}`.")),
    }
}

fn resolve_selector_metric_value(
    key: &str,
    args: &[CoreVerifyValue],
    bundle: &ArtifactBundle,
    manifest: &ModelManifest,
) -> Result<AuthorVerifyResolvedValue, String> {
    if args.len() != 1 {
        return Err("Selector metrics expect exactly one selector argument.".to_string());
    }
    let selector = verify_symbol_like(&args[0])
        .ok_or_else(|| "Selector metric argument must be symbol or text.".to_string())?;
    let anchors = resolve_distance_selector_anchors(selector, bundle, manifest)?;

    match key {
        "axis" => Ok(AuthorVerifyResolvedValue::Text(axis_name(selector_axis(
            selector, &anchors,
        )?)?)),
        "extent-x" => Ok(AuthorVerifyResolvedValue::Number(
            selector_extents(selector, &anchors)?[0],
        )),
        "extent-y" => Ok(AuthorVerifyResolvedValue::Number(
            selector_extents(selector, &anchors)?[1],
        )),
        "extent-z" => Ok(AuthorVerifyResolvedValue::Number(
            selector_extents(selector, &anchors)?[2],
        )),
        "center-x" => Ok(AuthorVerifyResolvedValue::Number(
            selector_center(selector, &anchors)?[0],
        )),
        "center-y" => Ok(AuthorVerifyResolvedValue::Number(
            selector_center(selector, &anchors)?[1],
        )),
        "center-z" => Ok(AuthorVerifyResolvedValue::Number(
            selector_center(selector, &anchors)?[2],
        )),
        other => Err(format!("Unsupported selector verify metric `{other}`.")),
    }
}

fn resolve_relation_metric_value(
    key: &str,
    args: &[CoreVerifyValue],
    bundle: &ArtifactBundle,
    manifest: &ModelManifest,
) -> Result<AuthorVerifyResolvedValue, String> {
    if args.len() != 2 {
        return Err("Relation metrics expect exactly two selector arguments.".to_string());
    }
    let left_selector = verify_symbol_like(&args[0])
        .ok_or_else(|| "Relation selector A must be symbol or text.".to_string())?;
    let right_selector = verify_symbol_like(&args[1])
        .ok_or_else(|| "Relation selector B must be symbol or text.".to_string())?;
    let left = resolve_distance_selector_anchors(left_selector, bundle, manifest)?;
    let right = resolve_distance_selector_anchors(right_selector, bundle, manifest)?;

    match key {
        "axis-angle" => {
            let left_axis = selector_axis(left_selector, &left)?;
            let right_axis = selector_axis(right_selector, &right)?;
            Ok(AuthorVerifyResolvedValue::Number(axis_angle_degrees(
                left_axis, right_axis,
            )?))
        }
        "center-delta-x" => Ok(AuthorVerifyResolvedValue::Number(
            selector_center(left_selector, &left)?[0] - selector_center(right_selector, &right)?[0],
        )),
        "center-delta-y" => Ok(AuthorVerifyResolvedValue::Number(
            selector_center(left_selector, &left)?[1] - selector_center(right_selector, &right)?[1],
        )),
        "center-delta-z" => Ok(AuthorVerifyResolvedValue::Number(
            selector_center(left_selector, &left)?[2] - selector_center(right_selector, &right)?[2],
        )),
        other => Err(format!("Unsupported relation verify metric `{other}`.")),
    }
}

fn resolve_distance_selector_anchors(
    selector: &str,
    bundle: &ArtifactBundle,
    manifest: &ModelManifest,
) -> Result<Vec<DistanceAnchor>, String> {
    let mut anchors = Vec::new();
    let mut visited_selectors = HashSet::new();
    let mut queue = VecDeque::from([selector.to_string()]);

    while let Some(current) = queue.pop_front() {
        if !visited_selectors.insert(current.clone()) {
            continue;
        }

        for part in &manifest.parts {
            if selector_matches_token(&current, &part.part_id) {
                if let Some(bounds) = &part.bounds {
                    if bounds_are_finite(bounds) {
                        anchors.push(DistanceAnchor::Bounds {
                            bounds: bounds.clone(),
                        });
                    } else {
                        return Err(format!(
                            "Verify selector `{current}` matched part '{}' but its bounds are not finite.",
                            part.part_id
                        ));
                    }
                } else {
                    return Err(format!(
                        "Verify selector `{current}` matched part '{}' but bounds evidence is missing.",
                        part.part_id
                    ));
                }
            }
        }

        for target in &manifest.selection_targets {
            if selector_matches_distance_target(&current, target) {
                collect_selection_target_geometry(
                    &current,
                    target,
                    bundle,
                    manifest,
                    &mut anchors,
                )?;
            }
        }

        for edge in &bundle.edge_targets {
            if selector_matches_token(&current, &edge.target_id)
                || edge
                    .durable_target_id
                    .as_deref()
                    .is_some_and(|value| selector_matches_token(&current, value))
                || edge
                    .canonical_target_id
                    .as_deref()
                    .is_some_and(|value| selector_matches_token(&current, value))
                || edge
                    .alias_ids
                    .iter()
                    .any(|alias| selector_matches_token(&current, alias))
            {
                anchors.push(DistanceAnchor::Segment {
                    start: [edge.start.x, edge.start.y, edge.start.z],
                    end: [edge.end.x, edge.end.y, edge.end.z],
                });
            }
        }

        for face in &bundle.face_targets {
            if selector_matches_token(&current, &face.target_id)
                || face
                    .durable_target_id
                    .as_deref()
                    .is_some_and(|value| selector_matches_token(&current, value))
                || face
                    .canonical_target_id
                    .as_deref()
                    .is_some_and(|value| selector_matches_token(&current, value))
                || face
                    .alias_ids
                    .iter()
                    .any(|alias| selector_matches_token(&current, alias))
            {
                if let Some(normal) = face.normal {
                    anchors.push(DistanceAnchor::Face {
                        point: [face.center.x, face.center.y, face.center.z],
                        normal,
                    });
                } else {
                    anchors.push(DistanceAnchor::Point {
                        point: [face.center.x, face.center.y, face.center.z],
                    });
                }
            }
        }

        if let Some((feature_id, output_id)) = current.split_once('.') {
            if let Some(correspondence_graph) = manifest.correspondence_graph.as_ref() {
                for edge in &correspondence_graph.edges {
                    if feature_output_matches(&edge.source, feature_id, output_id)
                        || feature_output_matches(&edge.target, feature_id, output_id)
                    {
                        for target_id in edge
                            .source
                            .target_ids
                            .iter()
                            .chain(edge.target.target_ids.iter())
                        {
                            queue.push_back(target_id.clone());
                        }
                    }
                }
            }
        }
    }

    if anchors.is_empty() {
        return Err(format!(
            "Unsupported verify selector `{selector}`: no manifest, correspondence, or mesh evidence matched."
        ));
    }
    Ok(anchors)
}

fn collect_selection_target_geometry(
    selector: &str,
    target: &crate::contracts::SelectionTarget,
    bundle: &ArtifactBundle,
    manifest: &ModelManifest,
    anchors: &mut Vec<DistanceAnchor>,
) -> Result<(), String> {
    let matched_edge = bundle.edge_targets.iter().find(|edge| {
        selector_matches_token(selector, &edge.target_id)
            || edge
                .durable_target_id
                .as_deref()
                .is_some_and(|value| selector_matches_token(selector, value))
            || edge
                .canonical_target_id
                .as_deref()
                .is_some_and(|value| selector_matches_token(selector, value))
            || edge
                .alias_ids
                .iter()
                .any(|alias| selector_matches_token(selector, alias))
    });
    let matched_face = bundle.face_targets.iter().find(|face| {
        selector_matches_token(selector, &face.target_id)
            || face
                .durable_target_id
                .as_deref()
                .is_some_and(|value| selector_matches_token(selector, value))
            || face
                .canonical_target_id
                .as_deref()
                .is_some_and(|value| selector_matches_token(selector, value))
            || face
                .alias_ids
                .iter()
                .any(|alias| selector_matches_token(selector, alias))
    });

    match target.kind {
        crate::contracts::SelectionTargetKind::Edge => {
            if let Some(edge) = matched_edge {
                anchors.push(DistanceAnchor::Segment {
                    start: [edge.start.x, edge.start.y, edge.start.z],
                    end: [edge.end.x, edge.end.y, edge.end.z],
                });
                Ok(())
            } else {
                Err(format!(
                    "Verify selector `{selector}` matched edge target '{}' but mesh edge evidence is missing.",
                    target.viewer_node_id
                ))
            }
        }
        crate::contracts::SelectionTargetKind::Face => {
            if let Some(face) = matched_face {
                if let Some(normal) = face.normal {
                    anchors.push(DistanceAnchor::Face {
                        point: [face.center.x, face.center.y, face.center.z],
                        normal,
                    });
                } else {
                    anchors.push(DistanceAnchor::Point {
                        point: [face.center.x, face.center.y, face.center.z],
                    });
                }
                Ok(())
            } else {
                Err(format!(
                    "Verify selector `{selector}` matched face target '{}' but mesh face evidence is missing.",
                    target.viewer_node_id
                ))
            }
        }
        _ => {
            let Some(part) = manifest
                .parts
                .iter()
                .find(|part| part.part_id == target.part_id)
            else {
                return Err(format!(
                    "Verify selector `{selector}` matched selection target '{}' but its part evidence is missing.",
                    target.viewer_node_id
                ));
            };
            let Some(bounds) = part.bounds.as_ref() else {
                return Err(format!(
                    "Verify selector `{selector}` matched selection target '{}' but part bounds evidence is missing.",
                    target.viewer_node_id
                ));
            };
            if !bounds_are_finite(bounds) {
                return Err(format!(
                    "Verify selector `{selector}` matched selection target '{}' but part bounds are not finite.",
                    target.viewer_node_id
                ));
            }
            anchors.push(DistanceAnchor::Bounds {
                bounds: bounds.clone(),
            });
            Ok(())
        }
    }
}

fn selector_matches_distance_target(
    selector: &str,
    target: &crate::contracts::SelectionTarget,
) -> bool {
    selector_matches_token(selector, target.target_id.as_deref().unwrap_or(""))
        || target
            .durable_target_id
            .as_deref()
            .is_some_and(|value| selector_matches_token(selector, value))
        || target
            .canonical_target_id
            .as_deref()
            .is_some_and(|value| selector_matches_token(selector, value))
        || target
            .alias_ids
            .iter()
            .any(|alias| selector_matches_token(selector, alias))
}

fn selector_matches_token(selector: &str, token: &str) -> bool {
    !token.trim().is_empty() && selector == token
}

fn feature_output_matches(
    output: &crate::contracts::FeatureOutputRef,
    feature_id: &str,
    output_id: &str,
) -> bool {
    output.feature_id == feature_id && output.output_id == output_id
}

fn min_pairwise_distance(left: &[DistanceAnchor], right: &[DistanceAnchor]) -> f64 {
    let mut best = f64::INFINITY;
    for left_anchor in left {
        for right_anchor in right {
            best = best.min(distance_between_anchors(left_anchor, right_anchor));
        }
    }
    best
}

fn selector_axis(selector: &str, anchors: &[DistanceAnchor]) -> Result<[f64; 3], String> {
    let mut axis: Option<[f64; 3]> = None;
    for anchor in anchors {
        let candidate = match anchor_axis(anchor)? {
            Some(candidate) => candidate,
            None => continue,
        };
        let candidate = normalize_axis(candidate).ok_or_else(|| {
            format!("Verify selector `{selector}` has zero-length axis evidence.")
        })?;
        if let Some(existing) = axis {
            let alignment = dot(existing, candidate).abs();
            if alignment < 0.999 {
                return Err(format!(
                    "Verify selector `{selector}` matched inconsistent axis evidence."
                ));
            }
        } else {
            axis = Some(candidate);
        }
    }
    axis.ok_or_else(|| format!("Verify selector `{selector}` has no axis evidence."))
}

fn anchor_axis(anchor: &DistanceAnchor) -> Result<Option<[f64; 3]>, String> {
    match anchor {
        DistanceAnchor::Point { .. } => Ok(None),
        DistanceAnchor::Face { normal, .. } => Ok(Some(*normal)),
        DistanceAnchor::Segment { start, end } => Ok(Some(subtract(*end, *start))),
        DistanceAnchor::Bounds { bounds } => bounds_axis(bounds).map(Some),
    }
}

fn bounds_axis(bounds: &ManifestBounds) -> Result<[f64; 3], String> {
    let extents = bounds_extents(bounds);
    let axis = dominant_axis_index(extents)?;
    Ok(match axis {
        0 => [1.0, 0.0, 0.0],
        1 => [0.0, 1.0, 0.0],
        2 => [0.0, 0.0, 1.0],
        _ => unreachable!(),
    })
}

fn axis_name(axis: [f64; 3]) -> Result<String, String> {
    let index = dominant_axis_index([axis[0].abs(), axis[1].abs(), axis[2].abs()])?;
    Ok(match index {
        0 => "x",
        1 => "y",
        2 => "z",
        _ => unreachable!(),
    }
    .to_string())
}

fn dominant_axis_index(values: [f64; 3]) -> Result<usize, String> {
    let mut order = [
        (0usize, values[0]),
        (1usize, values[1]),
        (2usize, values[2]),
    ];
    order.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if !order[0].1.is_finite() || order[0].1 <= f64::EPSILON {
        return Err("Axis evidence has no finite positive extent.".to_string());
    }
    if (order[0].1 - order[1].1).abs() <= 1.0e-9 {
        return Err("Axis evidence is ambiguous; top two extents tie.".to_string());
    }
    Ok(order[0].0)
}

fn selector_extents(selector: &str, anchors: &[DistanceAnchor]) -> Result<[f64; 3], String> {
    let bounds = union_anchor_bounds(selector, anchors)?;
    Ok(bounds_extents(&bounds))
}

fn selector_center(selector: &str, anchors: &[DistanceAnchor]) -> Result<[f64; 3], String> {
    let bounds = union_anchor_bounds(selector, anchors)?;
    Ok(bounds_center(&bounds))
}

fn union_anchor_bounds(
    selector: &str,
    anchors: &[DistanceAnchor],
) -> Result<ManifestBounds, String> {
    let mut bounds_iter = anchors.iter().map(anchor_bounds);
    let Some(first) = bounds_iter.next() else {
        return Err(format!(
            "Verify selector `{selector}` has no geometry evidence."
        ));
    };
    let bounds = bounds_iter.fold(first, |acc, bounds| union_two_bounds(&acc, &bounds));
    if !bounds_are_finite(&bounds) {
        return Err(format!(
            "Verify selector `{selector}` resolved non-finite bounds evidence."
        ));
    }
    Ok(bounds)
}

fn anchor_bounds(anchor: &DistanceAnchor) -> ManifestBounds {
    match anchor {
        DistanceAnchor::Point { point } | DistanceAnchor::Face { point, .. } => ManifestBounds {
            x_min: point[0],
            y_min: point[1],
            z_min: point[2],
            x_max: point[0],
            y_max: point[1],
            z_max: point[2],
        },
        DistanceAnchor::Segment { start, end } => ManifestBounds {
            x_min: start[0].min(end[0]),
            y_min: start[1].min(end[1]),
            z_min: start[2].min(end[2]),
            x_max: start[0].max(end[0]),
            y_max: start[1].max(end[1]),
            z_max: start[2].max(end[2]),
        },
        DistanceAnchor::Bounds { bounds } => bounds.clone(),
    }
}

fn union_two_bounds(left: &ManifestBounds, right: &ManifestBounds) -> ManifestBounds {
    ManifestBounds {
        x_min: left.x_min.min(right.x_min),
        y_min: left.y_min.min(right.y_min),
        z_min: left.z_min.min(right.z_min),
        x_max: left.x_max.max(right.x_max),
        y_max: left.y_max.max(right.y_max),
        z_max: left.z_max.max(right.z_max),
    }
}

fn bounds_extents(bounds: &ManifestBounds) -> [f64; 3] {
    [
        bounds.x_max - bounds.x_min,
        bounds.y_max - bounds.y_min,
        bounds.z_max - bounds.z_min,
    ]
}

fn bounds_center(bounds: &ManifestBounds) -> [f64; 3] {
    [
        (bounds.x_min + bounds.x_max) * 0.5,
        (bounds.y_min + bounds.y_max) * 0.5,
        (bounds.z_min + bounds.z_max) * 0.5,
    ]
}

fn normalize_axis(value: [f64; 3]) -> Option<[f64; 3]> {
    let length = norm(value);
    (length > f64::EPSILON && length.is_finite()).then_some(scale(value, 1.0 / length))
}

fn axis_angle_degrees(left: [f64; 3], right: [f64; 3]) -> Result<f64, String> {
    let left =
        normalize_axis(left).ok_or_else(|| "Left selector axis is zero-length.".to_string())?;
    let right =
        normalize_axis(right).ok_or_else(|| "Right selector axis is zero-length.".to_string())?;
    let cosine = dot(left, right).abs().clamp(0.0, 1.0);
    Ok(cosine.acos().to_degrees())
}

fn distance_between_anchors(left: &DistanceAnchor, right: &DistanceAnchor) -> f64 {
    match (left, right) {
        (
            DistanceAnchor::Bounds { bounds: left, .. },
            DistanceAnchor::Bounds { bounds: right, .. },
        ) => aabb_distance(left, right),
        (DistanceAnchor::Point { point: left, .. }, DistanceAnchor::Point { point: right, .. }) => {
            point_distance(*left, *right)
        }
        (DistanceAnchor::Face { point: left, .. }, DistanceAnchor::Face { point: right, .. }) => {
            point_distance(*left, *right)
        }
        (DistanceAnchor::Point { point: left, .. }, DistanceAnchor::Face { point: right, .. })
        | (DistanceAnchor::Face { point: left, .. }, DistanceAnchor::Point { point: right, .. }) => {
            point_distance(*left, *right)
        }
        (DistanceAnchor::Point { point, .. }, DistanceAnchor::Segment { start, end, .. })
        | (DistanceAnchor::Face { point, .. }, DistanceAnchor::Segment { start, end, .. })
        | (DistanceAnchor::Segment { start, end, .. }, DistanceAnchor::Point { point, .. }) => {
            point_segment_distance(*point, *start, *end)
        }
        (DistanceAnchor::Segment { start, end, .. }, DistanceAnchor::Face { point, .. }) => {
            point_segment_distance(*point, *start, *end)
        }
        (
            DistanceAnchor::Segment {
                start: left_start,
                end: left_end,
                ..
            },
            DistanceAnchor::Segment {
                start: right_start,
                end: right_end,
                ..
            },
        ) => segment_segment_distance(*left_start, *left_end, *right_start, *right_end),
        (DistanceAnchor::Bounds { bounds, .. }, DistanceAnchor::Point { point, .. })
        | (DistanceAnchor::Bounds { bounds, .. }, DistanceAnchor::Face { point, .. })
        | (DistanceAnchor::Point { point, .. }, DistanceAnchor::Bounds { bounds, .. })
        | (DistanceAnchor::Face { point, .. }, DistanceAnchor::Bounds { bounds, .. }) => {
            point_aabb_distance(*point, bounds)
        }
        (DistanceAnchor::Bounds { bounds, .. }, DistanceAnchor::Segment { start, end, .. })
        | (DistanceAnchor::Segment { start, end, .. }, DistanceAnchor::Bounds { bounds, .. }) => {
            segment_aabb_distance(*start, *end, bounds)
        }
    }
}

fn aabb_distance(a: &ManifestBounds, b: &ManifestBounds) -> f64 {
    let dx = axis_distance(a.x_min, a.x_max, b.x_min, b.x_max);
    let dy = axis_distance(a.y_min, a.y_max, b.y_min, b.y_max);
    let dz = axis_distance(a.z_min, a.z_max, b.z_min, b.z_max);
    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn point_distance(left: [f64; 3], right: [f64; 3]) -> f64 {
    let dx = left[0] - right[0];
    let dy = left[1] - right[1];
    let dz = left[2] - right[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn point_segment_distance(point: [f64; 3], start: [f64; 3], end: [f64; 3]) -> f64 {
    let segment = subtract(end, start);
    let length_sq = dot(segment, segment);
    if length_sq <= f64::EPSILON {
        return point_distance(point, start);
    }
    let t = clamp01(dot(subtract(point, start), segment) / length_sq);
    point_distance(point, add(start, scale(segment, t)))
}

fn segment_segment_distance(
    left_start: [f64; 3],
    left_end: [f64; 3],
    right_start: [f64; 3],
    right_end: [f64; 3],
) -> f64 {
    let u = subtract(left_end, left_start);
    let v = subtract(right_end, right_start);
    let w = subtract(left_start, right_start);
    let a = dot(u, u);
    let b = dot(u, v);
    let c = dot(v, v);
    let d = dot(u, w);
    let e = dot(v, w);
    let mut s_numer;
    let mut s_denom = a * c - b * b;
    let mut t_numer;
    let mut t_denom = s_denom;
    const EPS: f64 = 1.0e-12;

    if a <= EPS && c <= EPS {
        return point_distance(left_start, right_start);
    }
    if a <= EPS {
        return point_segment_distance(left_start, right_start, right_end);
    }
    if c <= EPS {
        return point_segment_distance(right_start, left_start, left_end);
    }

    if s_denom.abs() <= EPS {
        s_numer = 0.0;
        s_denom = 1.0;
        t_numer = e;
        t_denom = c;
    } else {
        s_numer = b * e - c * d;
        t_numer = a * e - b * d;

        if s_numer < 0.0 {
            s_numer = 0.0;
            t_numer = e;
            t_denom = c;
        } else if s_numer > s_denom {
            s_numer = s_denom;
            t_numer = e + b;
            t_denom = c;
        }
    }

    if t_numer < 0.0 {
        t_numer = 0.0;
        if -d < 0.0 {
            s_numer = 0.0;
        } else if -d > a {
            s_numer = s_denom;
        } else {
            s_numer = -d;
            s_denom = a;
        }
    } else if t_numer > t_denom {
        t_numer = t_denom;
        if (-d + b) < 0.0 {
            s_numer = 0.0;
        } else if (-d + b) > a {
            s_numer = s_denom;
        } else {
            s_numer = -d + b;
            s_denom = a;
        }
    }

    let sc = if s_numer.abs() <= EPS {
        0.0
    } else {
        s_numer / s_denom
    };
    let tc = if t_numer.abs() <= EPS {
        0.0
    } else {
        t_numer / t_denom
    };
    let delta = subtract(add(w, scale(u, sc)), scale(v, tc));
    norm(delta)
}

fn point_aabb_distance(point: [f64; 3], bounds: &ManifestBounds) -> f64 {
    let dx = axis_point_distance(point[0], bounds.x_min, bounds.x_max);
    let dy = axis_point_distance(point[1], bounds.y_min, bounds.y_max);
    let dz = axis_point_distance(point[2], bounds.z_min, bounds.z_max);
    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn segment_aabb_distance(start: [f64; 3], end: [f64; 3], bounds: &ManifestBounds) -> f64 {
    if segment_intersects_aabb(start, end, bounds) {
        return 0.0;
    }

    let mut best = f64::INFINITY;
    for point in segment_aabb_candidates(start, end, bounds) {
        best = best.min(point_aabb_distance(point, bounds));
    }
    best
}

fn segment_aabb_candidates(
    start: [f64; 3],
    end: [f64; 3],
    bounds: &ManifestBounds,
) -> Vec<[f64; 3]> {
    let mut points = vec![start, end];
    let delta = subtract(end, start);
    let planes = [
        (0usize, bounds.x_min),
        (0usize, bounds.x_max),
        (1usize, bounds.y_min),
        (1usize, bounds.y_max),
        (2usize, bounds.z_min),
        (2usize, bounds.z_max),
    ];
    for (axis, plane) in planes {
        let denom = delta[axis];
        if denom.abs() <= f64::EPSILON {
            continue;
        }
        let t = (plane - start[axis]) / denom;
        if !(0.0..=1.0).contains(&t) {
            continue;
        }
        let mut candidate = add(start, scale(delta, t));
        candidate[axis] = plane;
        points.push(candidate);
    }
    points
}

fn segment_intersects_aabb(start: [f64; 3], end: [f64; 3], bounds: &ManifestBounds) -> bool {
    let delta = subtract(end, start);
    let mut t_min: f64 = 0.0;
    let mut t_max: f64 = 1.0;
    for (origin, direction, min, max) in [
        (start[0], delta[0], bounds.x_min, bounds.x_max),
        (start[1], delta[1], bounds.y_min, bounds.y_max),
        (start[2], delta[2], bounds.z_min, bounds.z_max),
    ] {
        if direction.abs() <= f64::EPSILON {
            if origin < min || origin > max {
                return false;
            }
            continue;
        }
        let inv = 1.0 / direction;
        let mut t1 = (min - origin) * inv;
        let mut t2 = (max - origin) * inv;
        if t1 > t2 {
            std::mem::swap(&mut t1, &mut t2);
        }
        t_min = t_min.max(t1);
        t_max = t_max.min(t2);
        if t_min > t_max {
            return false;
        }
    }
    true
}

fn axis_distance(min_a: f64, max_a: f64, min_b: f64, max_b: f64) -> f64 {
    if max_a < min_b {
        min_b - max_a
    } else if max_b < min_a {
        min_a - max_b
    } else {
        0.0
    }
}

fn axis_point_distance(value: f64, min: f64, max: f64) -> f64 {
    if value < min {
        min - value
    } else if value > max {
        value - max
    } else {
        0.0
    }
}

fn subtract(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
}

fn add(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [left[0] + right[0], left[1] + right[1], left[2] + right[2]]
}

fn scale(value: [f64; 3], factor: f64) -> [f64; 3] {
    [value[0] * factor, value[1] * factor, value[2] * factor]
}

fn dot(left: [f64; 3], right: [f64; 3]) -> f64 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

fn norm(value: [f64; 3]) -> f64 {
    dot(value, value).sqrt()
}

fn clamp01(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

fn resolve_manifest_metric_value(
    key: &str,
    bundle: &ArtifactBundle,
    metrics: &AuthorVerificationMetrics,
) -> Result<AuthorVerifyResolvedValue, String> {
    match key {
        "has-step" => Ok(AuthorVerifyResolvedValue::Boolean(
            bundle.export_artifacts.iter().any(|artifact| {
                artifact.format.eq_ignore_ascii_case("step")
                    || artifact.format.eq_ignore_ascii_case("stp")
                    || artifact.role.eq_ignore_ascii_case("step")
            }),
        )),
        "has-model-stl" | "has-preview-stl" => Ok(AuthorVerifyResolvedValue::Boolean(
            !bundle.model_stl_path.trim().is_empty(),
        )),
        "edge-target-count" => Ok(AuthorVerifyResolvedValue::Number(
            bundle.edge_targets.len() as f64
        )),
        "face-target-count" => Ok(AuthorVerifyResolvedValue::Number(
            bundle.face_targets.len() as f64
        )),
        "export-format-count" => {
            let mut formats = BTreeMap::<String, ()>::new();
            for artifact in &bundle.export_artifacts {
                formats.insert(artifact.format.to_ascii_lowercase(), ());
            }
            Ok(AuthorVerifyResolvedValue::Number(formats.len() as f64))
        }
        "part-count" => Ok(AuthorVerifyResolvedValue::Number(
            metrics.manifest.part_count as f64,
        )),
        other => Err(format!("Unsupported manifest verify metric `{other}`.")),
    }
}

fn resolve_stl_metric_value(
    key: &str,
    args: &[CoreVerifyValue],
    bundle: &ArtifactBundle,
    manifest: &ModelManifest,
    metrics: &AuthorVerificationMetrics,
) -> Result<AuthorVerifyResolvedValue, String> {
    if matches!(
        key,
        "bed-contact-area-ratio" | "bed-contact-x-span-ratio" | "bed-contact-y-span-ratio"
    ) {
        let path = resolve_stl_metric_path(args, bundle, manifest)?;
        let contact = super::structural_verification::model_stl_bed_contact_summary(&path)
            .map_err(|error| error.to_string())?;
        let value = match key {
            "bed-contact-area-ratio" => contact.area_ratio,
            "bed-contact-x-span-ratio" => contact.x_span_ratio,
            "bed-contact-y-span-ratio" => contact.y_span_ratio,
            _ => unreachable!(),
        };
        return Ok(AuthorVerifyResolvedValue::Number(value));
    }
    if !args.is_empty() {
        return Err(format!(
            "STL verify metric `{key}` does not accept selector arguments."
        ));
    }
    let structural = metrics
        .structural
        .as_ref()
        .ok_or_else(|| "Structural verification evidence missing.".to_string())?;
    match key {
        "triangle-count" => structural
            .model_stl_triangle_count
            .map(|value| AuthorVerifyResolvedValue::Number(value as f64))
            .ok_or_else(|| "Triangle count missing from structural evidence.".to_string()),
        "connected-component-count" => structural
            .model_stl_component_count
            .map(|value| AuthorVerifyResolvedValue::Number(value as f64))
            .ok_or_else(|| {
                "Connected component count missing from structural evidence.".to_string()
            }),
        "non-manifold-edge-count" => structural
            .model_stl_non_manifold_edge_count
            .map(|value| AuthorVerifyResolvedValue::Number(value as f64))
            .ok_or_else(|| "Non-manifold edge count missing from structural evidence.".to_string()),
        "overhang-face-count" => structural
            .model_stl_overhang_triangle_count
            .map(|value| AuthorVerifyResolvedValue::Number(value as f64))
            .ok_or_else(|| "Overhang face count missing from structural evidence.".to_string()),
        other => Err(format!("Unsupported stl verify metric `{other}`.")),
    }
}

fn resolve_stl_metric_path(
    args: &[CoreVerifyValue],
    bundle: &ArtifactBundle,
    manifest: &ModelManifest,
) -> Result<PathBuf, String> {
    match args {
        [] => Ok(PathBuf::from(&bundle.model_stl_path)),
        [selector] => {
            let part_id = verify_symbol_like(selector)
                .ok_or_else(|| "STL part selector must be symbol or text.".to_string())?;
            let part = manifest
                .parts
                .iter()
                .find(|part| part.part_id == part_id)
                .ok_or_else(|| format!("STL part selector `{part_id}` did not resolve."))?;
            let asset_path = part
                .viewer_asset_path
                .as_deref()
                .ok_or_else(|| format!("Part `{part_id}` has no viewer asset for STL analysis."))?;
            let asset_path = Path::new(asset_path);
            Ok(if asset_path.is_absolute() {
                asset_path.to_path_buf()
            } else {
                Path::new(&bundle.manifest_path)
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(asset_path)
            })
        }
        _ => Err("STL bed-contact metrics accept at most one part selector.".to_string()),
    }
}

fn compare_values(
    actual: &AuthorVerifyResolvedValue,
    op: &str,
    expected: &AuthorVerifyResolvedValue,
) -> bool {
    match (actual, expected) {
        (AuthorVerifyResolvedValue::Number(left), AuthorVerifyResolvedValue::Number(right)) => {
            match op {
                "=" => left == right,
                "!=" => left != right,
                ">" => left > right,
                ">=" => left >= right,
                "<" => left < right,
                "<=" => left <= right,
                _ => false,
            }
        }
        (AuthorVerifyResolvedValue::Boolean(left), AuthorVerifyResolvedValue::Boolean(right)) => {
            (matches!(op, "=") && left == right) || (matches!(op, "!=") && left != right)
        }
        (AuthorVerifyResolvedValue::Text(left), AuthorVerifyResolvedValue::Text(right)) => {
            (matches!(op, "=") && left == right) || (matches!(op, "!=") && left != right)
        }
        _ => false,
    }
}

fn describe_value(value: &AuthorVerifyResolvedValue) -> String {
    match value {
        AuthorVerifyResolvedValue::Number(number) => number.to_string(),
        AuthorVerifyResolvedValue::Boolean(flag) => flag.to_string(),
        AuthorVerifyResolvedValue::Text(text) => format!("\"{text}\""),
    }
}

fn verify_symbol_like(value: &CoreVerifyValue) -> Option<&str> {
    match value {
        CoreVerifyValue::Symbol(value) | CoreVerifyValue::Text(value) => Some(value.as_str()),
        _ => None,
    }
}

fn resolve_literal(value: &CoreVerifyValue) -> Option<AuthorVerifyResolvedValue> {
    match value {
        CoreVerifyValue::Number(value) => Some(AuthorVerifyResolvedValue::Number(*value)),
        CoreVerifyValue::Boolean(value) => Some(AuthorVerifyResolvedValue::Boolean(*value)),
        CoreVerifyValue::Text(value) | CoreVerifyValue::Symbol(value) => {
            // `(= false)` / `(= true)` carry the boolean through as a symbol;
            // resolve it so boolean metrics (e.g. `has-step`) compare as
            // booleans instead of always mismatching a Text literal.
            match value.as_str() {
                "true" => Some(AuthorVerifyResolvedValue::Boolean(true)),
                "false" => Some(AuthorVerifyResolvedValue::Boolean(false)),
                _ => Some(AuthorVerifyResolvedValue::Text(value.clone())),
            }
        }
        CoreVerifyValue::List(_) => None,
    }
}

fn verify_error(
    clause_index: usize,
    metric_alias: Option<String>,
    message: &str,
) -> AuthorVerifyCheckResult {
    AuthorVerifyCheckResult {
        clause_index,
        status: AuthorVerifyCheckStatus::Error,
        severity: AuthoredVerifySeverity::Error,
        intent: None,
        condition: None,
        condition_result: None,
        skip_reason: None,
        metric_alias,
        metric_source: None,
        metric_key: None,
        comparator: None,
        expected: None,
        actual: None,
        message: message.to_string(),
    }
}

fn bounds_are_finite(bounds: &ManifestBounds) -> bool {
    [
        bounds.x_min,
        bounds.y_min,
        bounds.z_min,
        bounds.x_max,
        bounds.y_max,
        bounds.z_max,
    ]
    .into_iter()
    .all(f64::is_finite)
}

fn sum_finite_values(values: impl Iterator<Item = f64>) -> Option<f64> {
    let mut sum = 0.0;
    let mut count = 0usize;
    for value in values.filter(|value| value.is_finite()) {
        sum += value;
        count += 1;
    }
    (count > 0).then_some(sum)
}

fn union_bounds<'a>(
    bounds_iter: impl Iterator<Item = &'a ManifestBounds>,
) -> Option<ManifestBounds> {
    let mut iter = bounds_iter.filter(|bounds| bounds_are_finite(bounds));
    let first = iter.next()?.clone();
    Some(iter.fold(first, |acc, bounds| ManifestBounds {
        x_min: acc.x_min.min(bounds.x_min),
        y_min: acc.y_min.min(bounds.y_min),
        z_min: acc.z_min.min(bounds.z_min),
        x_max: acc.x_max.max(bounds.x_max),
        y_max: acc.y_max.max(bounds.y_max),
        z_max: acc.z_max.max(bounds.z_max),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{
        CorrespondenceEdge, CorrespondenceGraph, DocumentMetadata, EngineKind, EnrichmentStatus,
        ExportArtifact, FeatureOutputRef, GeometryBackend, ManifestEnrichmentState,
        ModelSourceKind, SourceLanguage, StructuralIssue, StructuralMetrics, VerifierStatus,
    };
    use crate::ecky_core_ir::{CoreVerifyClause, CoreVerifySection, CoreVerifyValue};
    use std::fs;
    use uuid::Uuid;

    #[test]
    #[ignore = "requires the prepared native OCCT runtime"]
    fn worm_farm_render_executes_its_authored_verification() {
        struct FarmResolver(std::path::PathBuf);
        impl crate::models::PathResolver for FarmResolver {
            fn app_config_dir(&self) -> std::path::PathBuf {
                self.0.join("config")
            }
            fn app_data_dir(&self) -> std::path::PathBuf {
                self.0.join("data")
            }
            fn resource_path(&self, _: &str) -> Option<std::path::PathBuf> {
                None
            }
        }
        let examples = [
            (
                include_str!("../../../docs/books/ecky-ir/examples/worm-farm/01-worm.ecky"),
                6,
            ),
            (
                include_str!(
                    "../../../docs/books/ecky-ir/examples/worm-farm/02-mushroom-house.ecky"
                ),
                1,
            ),
            (
                include_str!("../../../docs/books/ecky-ir/examples/worm-farm/03-rake.ecky"),
                1,
            ),
            (
                include_str!("../../../docs/books/ecky-ir/examples/worm-farm/04-tractor.ecky"),
                2,
            ),
            (
                include_str!("../../../docs/books/ecky-ir/examples/worm-farm/06-farm.ecky"),
                2,
            ),
            (
                include_str!(
                    "../../../docs/books/ecky-ir/examples/worm-farm/tractor-fit-coupon.ecky"
                ),
                1,
            ),
            (
                include_str!(
                    "../../../docs/books/ecky-ir/examples/worm-farm/worm-joint-coupon.ecky"
                ),
                1,
            ),
        ];
        for (source, checks) in examples {
            let root = std::env::temp_dir().join(format!("ecky-farm-{}", Uuid::new_v4()));
            fs::create_dir_all(&root).expect("test directory");
            let resolver = FarmResolver(root.clone());
            let bundle = crate::services::render::render_cli_ecky(
                source,
                &DesignParams::new(),
                GeometryBackend::EckyRust,
                None,
                &resolver,
            )
            .expect("render book example");
            let manifest = crate::model_runtime::read_model_manifest(&resolver, &bundle.model_id)
                .expect("read native manifest");
            let result = verify_structure_with_author_verification(&bundle, &manifest);
            let evidence = serde_json::to_string_pretty(&result).expect("verification evidence");
            println!("{evidence}");
            let _ = fs::remove_dir_all(root);
            assert_eq!(result.authored_verify_checks.len(), checks, "{evidence}");
            assert!(
                result.authored_verify_checks.iter().all(
                    |check| check.status == crate::contracts::AuthoredVerifyCheckStatus::Passed
                ),
                "{evidence}"
            );
            assert!(result.passed, "{evidence}");
        }
    }

    fn bounds(
        x_min: f64,
        y_min: f64,
        z_min: f64,
        x_max: f64,
        y_max: f64,
        z_max: f64,
    ) -> ManifestBounds {
        ManifestBounds {
            x_min,
            y_min,
            z_min,
            x_max,
            y_max,
            z_max,
        }
    }

    fn sample_manifest() -> ModelManifest {
        ModelManifest {
            geometry_provenance: None,
            component_import_origins: Vec::new(),
            component_placement_evidence: Vec::new(),
            schema_version: 1,
            model_id: "model-1".to_string(),
            source_kind: ModelSourceKind::Generated,
            source_digest: None,
            core_digest: None,
            ast_schema_version: None,
            engine_kind: EngineKind::Build123d,
            source_language: SourceLanguage::Build123d,
            geometry_backend: GeometryBackend::Build123d,
            document: DocumentMetadata {
                document_name: "sample".to_string(),
                document_label: "Sample".to_string(),
                source_path: None,
                object_count: 2,
                warnings: vec![],
            },
            parts: vec![
                crate::contracts::PartBinding {
                    part_id: "base".to_string(),
                    freecad_object_name: "Base".to_string(),
                    label: "Base".to_string(),
                    kind: "solid".to_string(),
                    semantic_role: Some("body".to_string()),
                    viewer_asset_path: Some("/tmp/base.glb".to_string()),
                    viewer_node_ids: vec!["node-base".to_string()],
                    parameter_keys: vec!["width".to_string()],
                    editable: true,
                    bounds: Some(bounds(0.0, 0.0, 0.0, 10.0, 8.0, 4.0)),
                    volume: Some(320.0),
                    area: Some(224.0),
                },
                crate::contracts::PartBinding {
                    part_id: "cap".to_string(),
                    freecad_object_name: "Cap".to_string(),
                    label: "Cap".to_string(),
                    kind: "solid".to_string(),
                    semantic_role: None,
                    viewer_asset_path: None,
                    viewer_node_ids: vec![],
                    parameter_keys: vec![],
                    editable: false,
                    bounds: Some(bounds(2.0, 1.0, 4.0, 8.0, 6.0, 7.0)),
                    volume: Some(90.0),
                    area: Some(126.0),
                },
            ],
            parameter_groups: vec![crate::contracts::ParameterGroup {
                group_id: "dimensions".to_string(),
                label: "Dimensions".to_string(),
                parameter_keys: vec!["width".to_string()],
                part_ids: vec!["base".to_string()],
                editable: true,
                presentation: None,
                order: Some(0),
            }],
            control_primitives: vec![],
            control_relations: vec![],
            control_views: vec![],
            preview_views: vec![],
            advisories: vec![],
            selection_targets: vec![crate::contracts::SelectionTarget {
                target_id: Some("face-1".to_string()),
                durable_target_id: None,
                canonical_target_id: None,
                alias_ids: vec![],
                part_id: "base".to_string(),
                viewer_node_id: "node-base".to_string(),
                label: "Top".to_string(),
                kind: crate::contracts::SelectionTargetKind::Face,
                editable: true,
                parameter_keys: vec!["width".to_string()],
                primitive_ids: vec![],
                view_ids: vec![],
            }],
            measurement_annotations: vec![crate::contracts::MeasurementAnnotation {
                annotation_id: "ann-1".to_string(),
                label: "Width".to_string(),
                basis: crate::contracts::MeasurementBasis::Outer,
                axis: crate::contracts::MeasurementAxis::X,
                parameter_keys: vec!["width".to_string()],
                primitive_ids: vec![],
                target_ids: vec!["face-1".to_string()],
                guide_id: None,
                explanation: None,
                formula_hint: None,
                source: crate::contracts::MeasurementAnnotationSource::Generated,
            }],
            tagged_anchors: std::collections::BTreeMap::new(),
            feature_graph: None,
            correspondence_graph: None,
            analysis_declarations: Vec::new(),
            warnings: vec!["minor warning".to_string()],
            enrichment_state: ManifestEnrichmentState {
                status: EnrichmentStatus::Accepted,
                proposals: vec![],
            },
        }
    }

    fn sample_bundle() -> ArtifactBundle {
        ArtifactBundle {
            geometry_provenance: None,
            component_dependency_lock: None,
            component_dependency_lock_digest: None,
            component_import_origins: Vec::new(),
            component_placement_evidence: Vec::new(),
            schema_version: 1,
            model_id: "model-1".to_string(),
            source_kind: ModelSourceKind::Generated,
            engine_kind: EngineKind::Build123d,
            source_language: SourceLanguage::Build123d,
            geometry_backend: GeometryBackend::Build123d,
            content_hash: "hash".to_string(),
            artifact_version: 1,
            fcstd_path: "/tmp/model.FCStd".to_string(),
            manifest_path: "/tmp/model-manifest.json".to_string(),
            macro_path: None,
            model_stl_path: "/tmp/model.stl".to_string(),
            viewer_assets: vec![],
            edge_targets: vec![crate::contracts::ViewerEdgeTarget {
                target_id: "edge-1".to_string(),
                durable_target_id: None,
                canonical_target_id: None,
                alias_ids: vec![],
                part_id: "base".to_string(),
                viewer_node_id: "node-base".to_string(),
                label: "Edge".to_string(),
                editable: true,
                start: crate::contracts::ViewerEdgePoint {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                end: crate::contracts::ViewerEdgePoint {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
            }],
            face_targets: vec![crate::contracts::ViewerFaceTarget {
                target_id: "face-1".to_string(),
                durable_target_id: None,
                canonical_target_id: None,
                alias_ids: vec![],
                part_id: "base".to_string(),
                viewer_node_id: "node-base".to_string(),
                label: "Face".to_string(),
                editable: true,
                center: crate::contracts::ViewerEdgePoint {
                    x: 0.5,
                    y: 0.5,
                    z: 1.0,
                },
                normal: Some([0.0, 0.0, 1.0]),
                area: Some(10.0),
            }],
            callout_anchors: vec![],
            measurement_guides: vec![],
            export_artifacts: vec![
                ExportArtifact {
                    geometry_provenance: None,
                    label: "STEP".to_string(),
                    format: "step".to_string(),
                    path: "/tmp/model.step".to_string(),
                    role: "step".to_string(),
                },
                ExportArtifact {
                    geometry_provenance: None,
                    label: "Model STL".to_string(),
                    format: "stl".to_string(),
                    path: "/tmp/model.stl".to_string(),
                    role: "preview".to_string(),
                },
            ],
        }
    }

    fn verify_clause(
        metric_expr: CoreVerifyValue,
        expect_expr: CoreVerifyValue,
    ) -> CoreVerifyClause {
        CoreVerifyClause {
            tag: CoreVerifySection {
                items: vec![
                    CoreVerifyValue::Symbol("front_entrance".to_string()),
                    CoreVerifyValue::Symbol("body.front_window_1".to_string()),
                ],
            },
            intent: None,
            severity: None,
            when: None,
            metric: CoreVerifySection {
                items: vec![CoreVerifyValue::Symbol("check".to_string()), metric_expr],
            },
            expect: CoreVerifySection {
                items: vec![CoreVerifyValue::Symbol("check".to_string()), expect_expr],
            },
        }
    }

    fn sample_structural_result(passed: bool) -> StructuralVerificationResult {
        StructuralVerificationResult {
            passed,
            summary: if passed {
                "looks good".to_string()
            } else {
                "issues found".to_string()
            },
            issues: if passed {
                vec![]
            } else {
                vec![
                    StructuralIssue {
                        code: "PART_DISCONNECTED".to_string(),
                        message: "cap disconnected".to_string(),
                        part_id: Some("cap".to_string()),
                        numeric_payload: Some(42.0),
                        diagnostic_context: None,
                    },
                    StructuralIssue {
                        code: "PART_DISCONNECTED".to_string(),
                        message: "cap still disconnected".to_string(),
                        part_id: Some("cap".to_string()),
                        numeric_payload: Some(43.0),
                        diagnostic_context: None,
                    },
                ]
            },
            authored_verify_checks: Vec::new(),
            metrics: StructuralMetrics {
                part_count: 2,
                model_stl_size_bytes: Some(2048),
                model_stl_triangle_count: Some(512),
                model_stl_component_count: Some(1),
                model_stl_non_manifold_edge_count: Some(0),
                model_stl_overhang_triangle_count: Some(10),
                model_stl_overhang_ratio: Some(0.02),
                total_volume: Some(410.0),
                total_area: Some(350.0),
                bbox: Some(bounds(0.0, 0.0, 0.0, 10.0, 8.0, 7.0)),
            },
            verifier_status: VerifierStatus::OkRustOnly,
            verifier_source: None,
        }
    }

    fn authored_bundle_with_source(source: &str) -> ArtifactBundle {
        let path =
            std::env::temp_dir().join(format!("ecky-authored-verify-{}.ecky", Uuid::new_v4()));
        fs::write(&path, source).expect("write source");
        let mut bundle = sample_bundle();
        bundle.engine_kind = EngineKind::EckyIrV0;
        bundle.source_language = SourceLanguage::EckyIrV0;
        bundle.geometry_backend = GeometryBackend::EckyRust;
        bundle.macro_path = Some(path.display().to_string());
        bundle
    }

    fn write_two_closed_tetrahedra(path: &std::path::Path) {
        let tetra = |x: f32| {
            [
                [[x, 0.0, 0.0], [x + 1.0, 0.0, 0.0], [x, 1.0, 0.0]],
                [[x, 0.0, 0.0], [x, 0.0, 1.0], [x + 1.0, 0.0, 0.0]],
                [[x, 0.0, 0.0], [x, 1.0, 0.0], [x, 0.0, 1.0]],
                [[x + 1.0, 0.0, 0.0], [x, 0.0, 1.0], [x, 1.0, 0.0]],
            ]
        };
        let triangles = tetra(0.0)
            .into_iter()
            .chain(tetra(10.0))
            .collect::<Vec<_>>();
        let mut bytes = vec![0_u8; 80];
        bytes.extend_from_slice(&(triangles.len() as u32).to_le_bytes());
        for triangle in triangles {
            for value in [0.0_f32; 3] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            for vertex in triangle {
                for value in vertex {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
            bytes.extend_from_slice(&0_u16.to_le_bytes());
        }
        fs::write(path, bytes).expect("write multipart STL");
    }

    fn write_bed_contact_fixture(path: &std::path::Path) {
        let triangles = [
            [[0.0_f32, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]],
            [[0.0_f32, 0.0, 1.0], [0.0, 10.0, 1.0], [10.0, 0.0, 1.0]],
        ];
        let mut bytes = vec![0_u8; 80];
        bytes.extend_from_slice(&(triangles.len() as u32).to_le_bytes());
        for triangle in triangles {
            for value in [0.0_f32; 3] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            for vertex in triangle {
                for value in vertex {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
            bytes.extend_from_slice(&0_u16.to_le_bytes());
        }
        fs::write(path, bytes).expect("write bed-contact STL");
    }

    #[test]
    fn authored_verify_supports_part_scoped_bed_contact_metrics() {
        let dir =
            std::env::temp_dir().join(format!("ecky-authored-bed-contact-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("create test dir");
        write_bed_contact_fixture(&dir.join("case.stl"));

        let mut bundle = sample_bundle();
        bundle.manifest_path = dir.join("manifest.json").display().to_string();
        let mut manifest = sample_manifest();
        manifest.parts.truncate(1);
        manifest.parts[0].viewer_asset_path = Some("case.stl".to_string());

        let metric = |key: &str| {
            CoreVerifyValue::List(vec![
                CoreVerifyValue::Symbol("stl".to_string()),
                CoreVerifyValue::Symbol(key.to_string()),
                CoreVerifyValue::Symbol("base".to_string()),
            ])
        };
        let less_than = |value: f64| {
            CoreVerifyValue::List(vec![
                CoreVerifyValue::Symbol("<".to_string()),
                CoreVerifyValue::Number(value),
            ])
        };
        let result = evaluate_author_verify_clauses(
            &[
                verify_clause(metric("bed-contact-area-ratio"), less_than(0.02)),
                verify_clause(metric("bed-contact-x-span-ratio"), less_than(0.2)),
                verify_clause(metric("bed-contact-y-span-ratio"), less_than(0.2)),
            ],
            &bundle,
            &manifest,
            Some(&sample_structural_result(true)),
        );

        assert!(result.passed, "Expected pass, got: {:?}", result.checks);
        assert!(result
            .checks
            .iter()
            .all(|check| check.status == AuthorVerifyCheckStatus::Passed));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn authored_component_count_is_structural_topology_contract() {
        let dir = std::env::temp_dir().join(format!(
            "ecky-authored-component-contract-{}",
            Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).expect("create test dir");
        let stl_path = dir.join("two-components.stl");
        write_two_closed_tetrahedra(&stl_path);

        let mut bundle = authored_bundle_with_source(
            "(model\n  (verify\n    (tag two-print-bodies)\n    (metric components (stl connected-component-count))\n    (expect components (= 2)))\n  (part body (box 1 1 1)))",
        );
        bundle.model_stl_path = stl_path.display().to_string();

        let mut manifest = sample_manifest();
        manifest.parts.truncate(1);
        manifest.parts[0].viewer_asset_path = None;

        let result = verify_structure_with_author_verification(&bundle, &manifest);

        assert!(result.passed, "Expected pass, got: {:?}", result.issues);
        assert_eq!(result.metrics.model_stl_component_count, Some(2));
        assert!(!result
            .issues
            .iter()
            .any(|issue| issue.code == "PREVIEW_STL_DISCONNECTED_COMPONENTS"));
        assert_eq!(
            result.authored_verify_checks[0].status,
            PublicAuthorVerifyCheckStatus::Passed
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn coverage_stats_report_ratio_and_completeness() {
        let stats = CoverageStats {
            present: 3,
            total: 4,
        };

        assert_eq!(stats.ratio(), Some(0.75));
        assert!(!stats.is_complete());
        assert_eq!(
            CoverageStats {
                present: 0,
                total: 0,
            }
            .ratio(),
            None
        );
    }

    #[test]
    fn collect_metrics_rolls_up_manifest_and_structural_data() {
        let metrics = collect_author_verification_metrics(
            &sample_manifest(),
            Some(&sample_structural_result(false)),
        );

        assert_eq!(metrics.manifest.part_count, 2);
        assert_eq!(metrics.manifest.editable_part_count, 1);
        assert_eq!(metrics.manifest.parameter_group_count, 1);
        assert_eq!(metrics.manifest.selection_target_count, 1);
        assert_eq!(metrics.manifest.measurement_annotation_count, 1);
        assert_eq!(metrics.manifest.warning_count, 1);
        assert_eq!(metrics.manifest.viewer_asset_coverage.present, 1);
        assert_eq!(metrics.manifest.viewer_asset_coverage.total, 2);
        assert!(!metrics.manifest.viewer_asset_coverage_complete());
        assert_eq!(metrics.manifest.total_volume_mm3, Some(410.0));
        assert_eq!(metrics.manifest.total_area_mm2, Some(350.0));
        assert_eq!(
            metrics.manifest.assembly_bounds,
            Some(bounds(0.0, 0.0, 0.0, 10.0, 8.0, 7.0))
        );

        let structural = metrics.structural.expect("structural metrics");
        assert_eq!(structural.issue_count, 2);
        assert_eq!(
            structural.issue_codes.get("PART_DISCONNECTED"),
            Some(&2usize)
        );
        assert!(!structural.is_passing());
    }

    #[test]
    fn readiness_requires_complete_geometry_and_passing_structural_result() {
        let passing = collect_author_verification_metrics(
            &sample_manifest(),
            Some(&sample_structural_result(true)),
        );
        assert!(passing.manifest.geometric_coverage_complete());
        assert!(passing.ready_for_author_verification());

        let mut incomplete_manifest = sample_manifest();
        incomplete_manifest.parts[1].bounds = None;
        let missing_bounds = collect_author_verification_metrics(
            &incomplete_manifest,
            Some(&sample_structural_result(true)),
        );
        assert!(!missing_bounds.manifest.geometric_coverage_complete());
        assert!(!missing_bounds.ready_for_author_verification());

        let no_structural = collect_author_verification_metrics(&sample_manifest(), None);
        assert!(!no_structural.ready_for_author_verification());
    }

    #[test]
    fn authored_verify_passes_on_manifest_and_stl_metrics() {
        let result = evaluate_author_verify_clauses(
            &[
                verify_clause(
                    CoreVerifyValue::List(vec![
                        CoreVerifyValue::Symbol("manifest".to_string()),
                        CoreVerifyValue::Symbol("has-step".to_string()),
                    ]),
                    CoreVerifyValue::List(vec![
                        CoreVerifyValue::Symbol("=".to_string()),
                        CoreVerifyValue::Boolean(true),
                    ]),
                ),
                verify_clause(
                    CoreVerifyValue::List(vec![
                        CoreVerifyValue::Symbol("stl".to_string()),
                        CoreVerifyValue::Symbol("triangle-count".to_string()),
                    ]),
                    CoreVerifyValue::List(vec![
                        CoreVerifyValue::Symbol(">".to_string()),
                        CoreVerifyValue::Number(100.0),
                    ]),
                ),
            ],
            &sample_bundle(),
            &sample_manifest(),
            Some(&sample_structural_result(true)),
        );

        assert!(result.passed);
        assert_eq!(result.checks.len(), 2);
        assert!(result
            .checks
            .iter()
            .all(|check| check.status == AuthorVerifyCheckStatus::Passed));
    }

    #[test]
    fn authored_verify_passes_on_part_bounds_clearance_metric() {
        let result = evaluate_author_verify_clauses(
            &[verify_clause(
                CoreVerifyValue::List(vec![
                    CoreVerifyValue::Symbol("clearance".to_string()),
                    CoreVerifyValue::Symbol("min-distance".to_string()),
                    CoreVerifyValue::Symbol("base".to_string()),
                    CoreVerifyValue::Symbol("cap".to_string()),
                ]),
                CoreVerifyValue::List(vec![
                    CoreVerifyValue::Symbol("=".to_string()),
                    CoreVerifyValue::Number(0.0),
                ]),
            )],
            &sample_bundle(),
            &sample_manifest(),
            Some(&sample_structural_result(true)),
        );

        assert!(result.passed);
        assert_eq!(result.checks[0].status, AuthorVerifyCheckStatus::Passed);
        assert_eq!(
            result.checks[0].actual,
            Some(AuthorVerifyResolvedValue::Number(0.0))
        );
    }

    #[test]
    fn authored_verify_passes_on_mesh_selector_clearance_metric() {
        let result = evaluate_author_verify_clauses(
            &[verify_clause(
                CoreVerifyValue::List(vec![
                    CoreVerifyValue::Symbol("clearance".to_string()),
                    CoreVerifyValue::Symbol("min-distance".to_string()),
                    CoreVerifyValue::Symbol("edge-1".to_string()),
                    CoreVerifyValue::Symbol("face-1".to_string()),
                ]),
                CoreVerifyValue::List(vec![
                    CoreVerifyValue::Symbol(">".to_string()),
                    CoreVerifyValue::Number(1.0),
                ]),
            )],
            &sample_bundle(),
            &sample_manifest(),
            Some(&sample_structural_result(true)),
        );

        assert!(result.passed);
        assert_eq!(result.checks[0].status, AuthorVerifyCheckStatus::Passed);
        let actual = match result.checks[0].actual.as_ref() {
            Some(AuthorVerifyResolvedValue::Number(value)) => *value,
            other => panic!("expected number actual, got {:?}", other),
        };
        assert!(actual > 1.0);
    }

    #[test]
    fn authored_verify_resolves_clearance_metric_through_correspondence_graph() {
        let mut manifest = sample_manifest();
        manifest.correspondence_graph = Some(CorrespondenceGraph {
            edges: vec![CorrespondenceEdge {
                edge_id: "edge-1".to_string(),
                source: FeatureOutputRef {
                    feature_id: "fitA".to_string(),
                    output_id: "out".to_string(),
                    target_ids: vec!["face-1".to_string()],
                },
                target: FeatureOutputRef {
                    feature_id: "fitB".to_string(),
                    output_id: "out".to_string(),
                    target_ids: vec![],
                },
                relation: "feeds".to_string(),
                source_ref: None,
            }],
        });

        let result = evaluate_author_verify_clauses(
            &[verify_clause(
                CoreVerifyValue::List(vec![
                    CoreVerifyValue::Symbol("clearance".to_string()),
                    CoreVerifyValue::Symbol("min-distance".to_string()),
                    CoreVerifyValue::Symbol("fitA.out".to_string()),
                    CoreVerifyValue::Symbol("edge-1".to_string()),
                ]),
                CoreVerifyValue::List(vec![
                    CoreVerifyValue::Symbol(">".to_string()),
                    CoreVerifyValue::Number(1.0),
                ]),
            )],
            &sample_bundle(),
            &manifest,
            Some(&sample_structural_result(true)),
        );

        assert!(result.passed);
        assert_eq!(result.checks[0].status, AuthorVerifyCheckStatus::Passed);
    }

    #[test]
    fn authored_verify_handles_correspondence_cycles_without_recursing_forever() {
        let mut manifest = sample_manifest();
        manifest.correspondence_graph = Some(CorrespondenceGraph {
            edges: vec![
                CorrespondenceEdge {
                    edge_id: "edge-1".to_string(),
                    source: FeatureOutputRef {
                        feature_id: "fitA".to_string(),
                        output_id: "out".to_string(),
                        target_ids: vec!["face-1".to_string()],
                    },
                    target: FeatureOutputRef {
                        feature_id: "fitB".to_string(),
                        output_id: "out".to_string(),
                        target_ids: vec!["fitB.out".to_string()],
                    },
                    relation: "feeds".to_string(),
                    source_ref: None,
                },
                CorrespondenceEdge {
                    edge_id: "edge-2".to_string(),
                    source: FeatureOutputRef {
                        feature_id: "fitB".to_string(),
                        output_id: "out".to_string(),
                        target_ids: vec!["fitA.out".to_string()],
                    },
                    target: FeatureOutputRef {
                        feature_id: "fitA".to_string(),
                        output_id: "out".to_string(),
                        target_ids: vec![],
                    },
                    relation: "feeds".to_string(),
                    source_ref: None,
                },
            ],
        });

        let result = evaluate_author_verify_clauses(
            &[verify_clause(
                CoreVerifyValue::List(vec![
                    CoreVerifyValue::Symbol("clearance".to_string()),
                    CoreVerifyValue::Symbol("min-distance".to_string()),
                    CoreVerifyValue::Symbol("fitA.out".to_string()),
                    CoreVerifyValue::Symbol("edge-1".to_string()),
                ]),
                CoreVerifyValue::List(vec![
                    CoreVerifyValue::Symbol(">".to_string()),
                    CoreVerifyValue::Number(1.0),
                ]),
            )],
            &sample_bundle(),
            &manifest,
            Some(&sample_structural_result(true)),
        );

        assert!(result.passed);
        assert_eq!(result.checks[0].status, AuthorVerifyCheckStatus::Passed);
    }

    #[test]
    fn authored_verify_fails_when_metric_expectation_misses() {
        let result = evaluate_author_verify_clauses(
            &[verify_clause(
                CoreVerifyValue::List(vec![
                    CoreVerifyValue::Symbol("stl".to_string()),
                    CoreVerifyValue::Symbol("non-manifold-edge-count".to_string()),
                ]),
                CoreVerifyValue::List(vec![
                    CoreVerifyValue::Symbol("=".to_string()),
                    CoreVerifyValue::Number(1.0),
                ]),
            )],
            &sample_bundle(),
            &sample_manifest(),
            Some(&sample_structural_result(true)),
        );

        assert!(!result.passed);
        assert_eq!(result.checks[0].status, AuthorVerifyCheckStatus::Failed);
        assert_eq!(
            result.checks[0].actual,
            Some(AuthorVerifyResolvedValue::Number(0.0))
        );
    }

    #[test]
    fn printed_thread_verify_set_is_reusable_and_covers_all_four_printability_checks() {
        let clauses = printed_thread_verify_clauses(2.0, 1.4, 45.0, 45.0);
        assert_eq!(clauses.len(), 4);
        let tags = clauses.iter().map(authored_verify_tag).collect::<Vec<_>>();
        assert_eq!(
            tags,
            vec![
                "printed_thread_single_solid",
                "printed_thread_manifold",
                "printed_thread_overhang",
                "printed_thread_turn_clearance",
            ]
        );

        let evaluation = evaluate_author_verify_clauses(
            &clauses,
            &sample_bundle(),
            &sample_manifest(),
            Some(&sample_structural_result(true)),
        );
        assert!(evaluation.passed, "{}", evaluation.summary);
    }

    #[test]
    fn printed_thread_overhang_clause_fails_shallow_flank_and_passes_looser_flank() {
        let shallow = printed_thread_verify_clauses(2.0, 1.4, 30.0, 45.0);
        let shallow_result = evaluate_author_verify_clauses(
            &shallow,
            &sample_bundle(),
            &sample_manifest(),
            Some(&sample_structural_result(true)),
        );
        assert!(!shallow_result.passed);
        assert_eq!(
            shallow_result.checks[2].status,
            AuthorVerifyCheckStatus::Failed
        );
        assert_eq!(
            shallow_result.checks[2].actual,
            Some(AuthorVerifyResolvedValue::Number(60.0))
        );

        let printable = printed_thread_verify_clauses(2.0, 1.4, 45.0, 45.0);
        let printable_result = evaluate_author_verify_clauses(
            &printable,
            &sample_bundle(),
            &sample_manifest(),
            Some(&sample_structural_result(true)),
        );
        assert!(printable_result.passed, "{}", printable_result.summary);
        assert_eq!(
            printable_result.checks[2].status,
            AuthorVerifyCheckStatus::Passed
        );
    }

    #[test]
    fn authored_verify_errors_when_clearance_selector_missing() {
        let result = evaluate_author_verify_clauses(
            &[verify_clause(
                CoreVerifyValue::List(vec![
                    CoreVerifyValue::Symbol("clearance".to_string()),
                    CoreVerifyValue::Symbol("min-distance".to_string()),
                    CoreVerifyValue::Symbol("missing_selector".to_string()),
                    CoreVerifyValue::Symbol("edge-1".to_string()),
                ]),
                CoreVerifyValue::List(vec![
                    CoreVerifyValue::Symbol(">=".to_string()),
                    CoreVerifyValue::Number(0.0),
                ]),
            )],
            &sample_bundle(),
            &sample_manifest(),
            Some(&sample_structural_result(true)),
        );

        assert!(!result.passed);
        assert_eq!(result.checks[0].status, AuthorVerifyCheckStatus::Error);
        assert!(result.checks[0].message.contains("missing_selector"));
    }

    #[test]
    fn authored_verify_checks_selector_axis_extent_and_center_from_part_bounds() {
        let result = evaluate_author_verify_clauses(
            &[
                verify_clause(
                    CoreVerifyValue::List(vec![
                        CoreVerifyValue::Symbol("selector".to_string()),
                        CoreVerifyValue::Symbol("axis".to_string()),
                        CoreVerifyValue::Symbol("base".to_string()),
                    ]),
                    CoreVerifyValue::List(vec![
                        CoreVerifyValue::Symbol("=".to_string()),
                        CoreVerifyValue::Text("x".to_string()),
                    ]),
                ),
                verify_clause(
                    CoreVerifyValue::List(vec![
                        CoreVerifyValue::Symbol("selector".to_string()),
                        CoreVerifyValue::Symbol("extent-y".to_string()),
                        CoreVerifyValue::Symbol("base".to_string()),
                    ]),
                    CoreVerifyValue::List(vec![
                        CoreVerifyValue::Symbol("=".to_string()),
                        CoreVerifyValue::Number(8.0),
                    ]),
                ),
                verify_clause(
                    CoreVerifyValue::List(vec![
                        CoreVerifyValue::Symbol("selector".to_string()),
                        CoreVerifyValue::Symbol("center-z".to_string()),
                        CoreVerifyValue::Symbol("base".to_string()),
                    ]),
                    CoreVerifyValue::List(vec![
                        CoreVerifyValue::Symbol("=".to_string()),
                        CoreVerifyValue::Number(2.0),
                    ]),
                ),
            ],
            &sample_bundle(),
            &sample_manifest(),
            Some(&sample_structural_result(true)),
        );

        assert!(result.passed);
        assert_eq!(result.checks.len(), 3);
    }

    #[test]
    fn authored_verify_checks_axis_angle_between_edge_and_face_selectors() {
        let result = evaluate_author_verify_clauses(
            &[verify_clause(
                CoreVerifyValue::List(vec![
                    CoreVerifyValue::Symbol("relation".to_string()),
                    CoreVerifyValue::Symbol("axis-angle".to_string()),
                    CoreVerifyValue::Symbol("edge-1".to_string()),
                    CoreVerifyValue::Symbol("face-1".to_string()),
                ]),
                CoreVerifyValue::List(vec![
                    CoreVerifyValue::Symbol("=".to_string()),
                    CoreVerifyValue::Number(90.0),
                ]),
            )],
            &sample_bundle(),
            &sample_manifest(),
            Some(&sample_structural_result(true)),
        );

        assert!(result.passed);
        assert_eq!(
            result.checks[0].actual,
            Some(AuthorVerifyResolvedValue::Number(90.0))
        );
    }

    #[test]
    fn authored_verify_checks_signed_center_delta_between_selectors() {
        let result = evaluate_author_verify_clauses(
            &[verify_clause(
                CoreVerifyValue::List(vec![
                    CoreVerifyValue::Symbol("relation".to_string()),
                    CoreVerifyValue::Symbol("center-delta-y".to_string()),
                    CoreVerifyValue::Symbol("cap".to_string()),
                    CoreVerifyValue::Symbol("base".to_string()),
                ]),
                CoreVerifyValue::List(vec![
                    CoreVerifyValue::Symbol("=".to_string()),
                    CoreVerifyValue::Number(-0.5),
                ]),
            )],
            &sample_bundle(),
            &sample_manifest(),
            Some(&sample_structural_result(true)),
        );

        assert!(result.passed);
        assert_eq!(
            result.checks[0].actual,
            Some(AuthorVerifyResolvedValue::Number(-0.5))
        );
    }

    #[test]
    fn authored_verify_errors_when_structural_evidence_missing() {
        let result = evaluate_author_verify_clauses(
            &[verify_clause(
                CoreVerifyValue::List(vec![
                    CoreVerifyValue::Symbol("stl".to_string()),
                    CoreVerifyValue::Symbol("triangle-count".to_string()),
                ]),
                CoreVerifyValue::List(vec![
                    CoreVerifyValue::Symbol(">".to_string()),
                    CoreVerifyValue::Number(0.0),
                ]),
            )],
            &sample_bundle(),
            &sample_manifest(),
            None,
        );

        assert!(!result.passed);
        assert_eq!(result.checks[0].status, AuthorVerifyCheckStatus::Error);
        assert!(result.checks[0]
            .message
            .contains("Structural verification evidence missing"));
    }

    #[test]
    fn merge_author_verification_into_structural_result_adds_failed_issue() {
        let bundle = authored_bundle_with_source(
            r#"
            (model
              (verify
                (tag body_shell)
                (metric check (manifest has-step))
                (expect check (= false)))
              (part body (box 10 10 10)))
            "#,
        );

        let result = merge_author_verification_into_structural_result(
            &bundle,
            &sample_manifest(),
            sample_structural_result(true),
        );

        assert!(!result.passed);
        assert!(result
            .issues
            .iter()
            .any(|issue| issue.code == "AUTHORED_VERIFY_FAILED"));
    }

    #[test]
    fn merge_author_verification_expands_reusable_printed_thread_set() {
        let bundle = authored_bundle_with_source(
            r#"
            (model
              (verify
                (tag printed_thread)
                (metric checks (thread printed-thread-set 2 1.4 45 45))
                (expect checks (= true)))
              (part screw
                (thread :radius 8 :pitch 2 :length 16 :depth 0.8 :crest 0.2 :flank 45deg)))
            "#,
        );

        let result = merge_author_verification_into_structural_result(
            &bundle,
            &sample_manifest(),
            sample_structural_result(true),
        );

        assert!(result.passed, "{}", result.summary);
        assert_eq!(result.authored_verify_checks.len(), 4);
        assert_eq!(
            result.authored_verify_checks[0].tag,
            "printed_thread_single_solid"
        );
        assert_eq!(
            result.authored_verify_checks[3].tag,
            "printed_thread_turn_clearance"
        );
    }

    #[test]
    fn authored_thread_core_volume_sanity_rejects_valid_but_hollow_result() {
        let bundle = authored_bundle_with_source(
            r#"
            (model
              (verify
                (tag thread_core_present)
                (metric ratio (thread core-volume-ratio 6 10))
                (expect ratio (>= 0.9)))
              (part screw
                (thread :radius 6 :pitch 3 :length 10 :depth 1.2)))
            "#,
        );
        let structural = sample_structural_result(true);
        assert_eq!(structural.metrics.model_stl_component_count, Some(1));
        assert_eq!(
            structural.metrics.model_stl_non_manifold_edge_count,
            Some(0)
        );

        let result = merge_author_verification_into_structural_result(
            &bundle,
            &sample_manifest(),
            structural,
        );

        assert!(!result.passed, "hollow result must fail geometric sanity");
        assert_eq!(result.authored_verify_checks.len(), 1);
        assert_eq!(result.authored_verify_checks[0].tag, "thread_core_present");
        assert_eq!(
            result.authored_verify_checks[0].status,
            crate::contracts::AuthoredVerifyCheckStatus::Failed
        );
        let ratio = match result.authored_verify_checks[0].actual {
            Some(crate::contracts::AuthoredVerifyValue::Number(value)) => value,
            ref other => panic!("expected numeric core-volume ratio, got {other:?}"),
        };
        assert!(ratio < 0.9, "hollow ratio unexpectedly passed: {ratio}");
    }

    #[test]
    fn merge_author_verification_into_structural_result_exposes_public_check_statuses() {
        let bundle = authored_bundle_with_source(
            r#"
            (model
              (verify
                (tag step_export)
                (metric check (stl triangle-count))
                (expect check (> 100)))
              (verify
                (tag bad_clearance)
                (metric check (stl non-manifold-edge-count))
                (expect check (= 1)))
              (part body (box 10 10 10)))
            "#,
        );

        let result = merge_author_verification_into_structural_result(
            &bundle,
            &sample_manifest(),
            sample_structural_result(true),
        );

        assert_eq!(result.authored_verify_checks.len(), 2);
        assert_eq!(result.authored_verify_checks[0].tag, "step_export");
        assert_eq!(
            result.authored_verify_checks[0].status,
            crate::contracts::AuthoredVerifyCheckStatus::Passed
        );
        assert_eq!(result.authored_verify_checks[1].tag, "bad_clearance");
        assert_eq!(
            result.authored_verify_checks[1].status,
            crate::contracts::AuthoredVerifyCheckStatus::Failed
        );
        assert_eq!(
            result.authored_verify_checks[1].stable_node_id.as_deref(),
            Some("verify:bad_clearance")
        );
        assert!(result.authored_verify_checks[1].message.contains("0 = 1"));
    }

    #[test]
    fn merge_author_verification_into_structural_result_adds_compile_error_issue() {
        let bundle = authored_bundle_with_source("(model (verify (tag body_shell)))");

        let result = merge_author_verification_into_structural_result(
            &bundle,
            &sample_manifest(),
            sample_structural_result(true),
        );

        assert!(!result.passed);
        assert!(result
            .issues
            .iter()
            .any(|issue| issue.code == "AUTHORED_VERIFY_ERROR"));
    }

    #[test]
    fn warning_failure_and_false_condition_are_visible_but_nonblocking() {
        let source = r#"
            (model
              (params (toggle assembly-preview #f))
              (verify
                (tag advisory-triangles)
                (intent "Triangle budget advisory")
                (severity warning)
                (metric triangles (stl triangle-count))
                (expect triangles (> 999999)))
              (verify
                (tag assembly-only)
                (when assembly-preview)
                (metric impossible (unsupported never-run))
                (expect impossible (= 1)))
              (part body (box 10 10 10)))
        "#;
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("compile");
        let params =
            DesignParams::from([("assembly-preview".to_string(), ParamValue::Boolean(false))]);
        let result = merge_loaded_author_verification_into_structural_result(
            &authored_bundle_with_source(source),
            &sample_manifest(),
            sample_structural_result(true),
            &program.constraints.verify_clauses,
            &params,
        );

        assert!(result.passed, "{}", result.summary);
        assert!(result.issues.is_empty());
        assert_eq!(
            result.authored_verify_checks[0].status,
            PublicAuthorVerifyCheckStatus::Failed
        );
        assert_eq!(
            result.authored_verify_checks[0].severity,
            AuthoredVerifySeverity::Warning
        );
        assert_eq!(
            result.authored_verify_checks[0].intent.as_deref(),
            Some("Triangle budget advisory")
        );
        assert_eq!(
            result.authored_verify_checks[1].status,
            PublicAuthorVerifyCheckStatus::Skipped
        );
        assert_eq!(
            result.authored_verify_checks[1].condition.as_deref(),
            Some("assembly-preview")
        );
        assert_eq!(
            result.authored_verify_checks[1].condition_result,
            Some(false)
        );
        assert!(result.authored_verify_checks[1].skip_reason.is_some());
    }

    #[test]
    fn invalid_warning_condition_remains_blocking_error() {
        let source = r#"
            (model
              (verify
                (tag invalid-condition)
                (severity warning)
                (when missing-toggle)
                (metric triangles (stl triangle-count))
                (expect triangles (> 0)))
              (part body (box 10 10 10)))
        "#;
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("compile");
        let result = merge_loaded_author_verification_into_structural_result(
            &authored_bundle_with_source(source),
            &sample_manifest(),
            sample_structural_result(true),
            &program.constraints.verify_clauses,
            &DesignParams::new(),
        );

        assert!(!result.passed);
        assert_eq!(
            result.authored_verify_checks[0].status,
            PublicAuthorVerifyCheckStatus::Error
        );
        assert!(result.authored_verify_checks[0]
            .message
            .contains("unknown parameter"));
        assert!(result
            .issues
            .iter()
            .any(|issue| issue.code == "AUTHORED_VERIFY_ERROR"));
    }
}
