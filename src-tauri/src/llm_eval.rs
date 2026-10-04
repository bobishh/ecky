//! File-backed LLM trajectory eval artifacts.
//!
//! Provider transports may be JSON, but persisted eval evidence is strict EDN
//! plus Markdown. No eval database or JSON log is created.

use regex::Regex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const EVAL_SCHEMA_VERSION: u32 = 2;
const MAX_PAYLOAD_STRING_CHARS: usize = 32 * 1024;
const MAX_TEXT_CHARS: usize = 16 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvalCase {
    pub case_id: String,
    pub objective: String,
    pub acceptance_criteria: Vec<String>,
    pub starting_version_id: Option<String>,
    pub starting_input_digest: Option<String>,
    pub expected_red_rounds: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EvalRoute {
    pub provider: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub prompt_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jev: Option<EvalJevRoute>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EvalJevRoute {
    pub policy_version: String,
    pub intent_confidence_threshold: f64,
    pub intent_margin_threshold: f64,
    pub model_confidence_threshold: f64,
    pub model_margin_threshold: f64,
    pub intent_confidence: f64,
    pub intent_probabilities: std::collections::BTreeMap<String, f64>,
    pub answer_requested: bool,
    pub answer_first: bool,
    pub model_ceiling: Option<String>,
    pub model_confidence: Option<f64>,
    pub model_probabilities: std::collections::BTreeMap<String, f64>,
    pub model_reason: String,
    pub context_truncated: bool,
    #[serde(default)]
    pub current_prompt_truncated: bool,
    pub classifier_input_tokens: Option<u64>,
    pub classifier_output_tokens: Option<u64>,
    pub classifier_model: Option<String>,
    pub classifier_latency_ms: Option<u64>,
    pub model_catalog_version: Option<String>,
    pub model_catalog_valid_until: Option<String>,
    #[serde(default = "unknown_native_tool_coverage")]
    pub native_tool_coverage: String,
}

pub fn unknown_native_tool_coverage() -> String {
    "unknown".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EvalUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub estimated_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvalTurnPolicy {
    pub intent: crate::provider_turn::ProviderTurnIntent,
    pub allows_tools: bool,
    pub allows_project_writes: bool,
    pub execution_mode: String,
    #[serde(default)]
    pub answer_first_required: bool,
}

impl EvalTurnPolicy {
    pub fn for_intent(intent: crate::provider_turn::ProviderTurnIntent) -> Self {
        Self::from_policy(crate::provider_turn::ProviderTurnPolicy::for_intent(intent))
    }

    fn from_policy(policy: crate::provider_turn::ProviderTurnPolicy) -> Self {
        Self {
            intent: policy.intent(),
            allows_tools: policy.allows_any_tool(),
            allows_project_writes: policy.allows_project_writes(),
            execution_mode: policy.execution_mode().to_string(),
            answer_first_required: policy.requires_answer_first(),
        }
    }

    fn runtime_policy(&self) -> crate::provider_turn::ProviderTurnPolicy {
        crate::provider_turn::ProviderTurnPolicy::routed(self.intent, self.answer_first_required)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EvalEventKind {
    Assistant,
    Tool,
    Result,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum EvalValue {
    Nil,
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
    List(Vec<EvalValue>),
    Map(Vec<EvalField>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EvalField {
    pub key: String,
    pub value: EvalValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EvalPayload {
    pub value: EvalValue,
    pub truncated: bool,
    pub sha256: String,
}

impl EvalPayload {
    pub fn new(value: Value) -> Self {
        let encoded = serde_json::to_vec(&value).unwrap_or_default();
        let sha256 = format!("sha256:{:x}", Sha256::digest(&encoded));
        let mut truncated = false;
        let value = sanitize_value(value, None, &mut truncated);
        Self {
            value,
            truncated,
            sha256,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EvalEvent {
    pub sequence: u64,
    pub step_index: Option<i64>,
    pub kind: EvalEventKind,
    pub state: String,
    pub name: Option<String>,
    pub summary: Option<String>,
    pub input: Option<EvalPayload>,
    pub output: Option<EvalPayload>,
    pub error: Option<String>,
    pub occurred_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvalVersionOutcome {
    pub version_id: String,
    pub input_digest: Option<String>,
    pub status: String,
    pub verification_passed: Option<bool>,
    pub raw_error: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EvalRun {
    pub schema_version: u32,
    pub run_id: String,
    pub case: EvalCase,
    pub thread_id: String,
    pub external_thread_id: String,
    pub turn_id: String,
    pub route: EvalRoute,
    pub prompt: String,
    pub started_at: i64,
    pub completed_at: i64,
    pub status: String,
    pub response: Option<String>,
    pub raw_error: Option<String>,
    #[serde(default)]
    pub turn_policy: Option<EvalTurnPolicy>,
    #[serde(default)]
    pub policy_violations: Vec<String>,
    pub events: Vec<EvalEvent>,
    pub versions: Vec<EvalVersionOutcome>,
    pub usage: Option<EvalUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EvalScores {
    pub completed: bool,
    pub terminal_success: bool,
    pub first_build_green: bool,
    pub red_to_green_repair: bool,
    pub tool_calls: u32,
    pub repeated_adjacent_tools: u32,
    pub policy_violations: u32,
    pub version_count: u32,
    pub red_versions: u32,
    pub unnecessary_versions: u32,
    pub duration_ms: u64,
    pub total_tokens: Option<u64>,
    pub estimated_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EvalRunFiles {
    pub run_dir: PathBuf,
    pub run_edn: PathBuf,
    pub trajectory_edn: PathBuf,
    pub report_md: PathBuf,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EvalRunSeed {
    pub run_id: String,
    pub thread_id: String,
    pub external_thread_id: String,
    pub provider: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub prompt_version: String,
    pub prompt: String,
    pub starting_version_id: Option<String>,
    pub starting_input_digest: Option<String>,
    pub expected_red_rounds: u32,
    pub turn_intent: crate::provider_turn::ProviderTurnIntent,
    pub answer_first_required: bool,
    pub jev_route: Option<EvalJevRoute>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EvalRunManifest {
    schema_version: u32,
    run_id: String,
    case: EvalCase,
    thread_id: String,
    external_thread_id: String,
    turn_id: String,
    route: EvalRoute,
    prompt: String,
    started_at: i64,
    completed_at: i64,
    status: String,
    response: Option<String>,
    raw_error: Option<String>,
    #[serde(default)]
    turn_policy: Option<EvalTurnPolicy>,
    #[serde(default)]
    policy_violations: Vec<String>,
    versions: Vec<EvalVersionOutcome>,
    usage: Option<EvalUsage>,
    scores: EvalScores,
    trajectory_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EvalTrajectoryFile {
    schema_version: u32,
    run_id: String,
    events: Vec<EvalEvent>,
}

/// Optional projection boundary. File artifacts remain authoritative.
pub trait EvalTelemetryExporter {
    fn export(&self, run: &EvalRun, scores: &EvalScores) -> Result<(), String>;
}

pub fn score_run(run: &EvalRun) -> EvalScores {
    let mut seen_steps = HashSet::new();
    let tool_events = run
        .events
        .iter()
        .filter(|event| event.kind == EvalEventKind::Tool)
        .filter(|event| {
            event
                .step_index
                .map(|step_index| seen_steps.insert(step_index))
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();
    let repeated_adjacent_tools = tool_events
        .windows(2)
        .filter(|pair| pair[0].name == pair[1].name)
        .count() as u32;
    let red_versions = run
        .versions
        .iter()
        .filter(|version| version.status == "error" || version.verification_passed == Some(false))
        .count() as u32;
    let first_build_green = run.versions.first().is_some_and(version_green);
    let final_version_green = run.versions.last().is_some_and(version_green);
    let terminal_success = run.status == "success";
    let completed = terminal_success && (run.versions.is_empty() || final_version_green);
    let red_to_green_repair = red_versions > 0 && final_version_green;
    let unnecessary_versions = red_versions.saturating_sub(run.case.expected_red_rounds);
    let duration_ms = run.completed_at.saturating_sub(run.started_at).max(0) as u64 * 1_000;
    let total_tokens =
        run.usage
            .as_ref()
            .and_then(|usage| match (usage.input_tokens, usage.output_tokens) {
                (None, None) => None,
                (input, output) => Some(input.unwrap_or(0).saturating_add(output.unwrap_or(0))),
            });
    EvalScores {
        completed,
        terminal_success,
        first_build_green,
        red_to_green_repair,
        tool_calls: tool_events.len() as u32,
        repeated_adjacent_tools,
        policy_violations: run.policy_violations.len() as u32,
        version_count: run.versions.len() as u32,
        red_versions,
        unnecessary_versions,
        duration_ms,
        total_tokens,
        estimated_cost_usd: run
            .usage
            .as_ref()
            .and_then(|usage| usage.estimated_cost_usd),
    }
}

pub fn evaluate_policy_violations(run: &EvalRun) -> Vec<String> {
    let Some(turn_policy) = run.turn_policy.as_ref() else {
        return Vec::new();
    };
    let policy = turn_policy.runtime_policy();
    let mut violations = Vec::new();
    let mut seen_steps = HashSet::new();
    for event in run
        .events
        .iter()
        .filter(|event| event.kind == EvalEventKind::Tool)
    {
        if event
            .step_index
            .is_some_and(|step_index| !seen_steps.insert(step_index))
        {
            continue;
        }
        let tool_name = event.name.as_deref().unwrap_or("unknown");
        if !policy.allows_mcp_tool(tool_name) {
            violations.push(format!(
                "tool `{tool_name}` is not allowed for {} intent",
                turn_policy.intent.as_str()
            ));
        }
    }
    if !policy.allows_project_writes() && !run.versions.is_empty() {
        violations.push(format!(
            "{} intent created {} immutable version(s)",
            turn_policy.intent.as_str(),
            run.versions.len()
        ));
    }
    violations
}

pub fn persist_run(root: &Path, run: &EvalRun) -> Result<EvalRunFiles, String> {
    validate_run(run)?;
    let run = sanitize_run(run.clone());
    let scores = score_run(&run);
    let run_dir = root.join("evals").join("runs").join(&run.run_id);
    fs::create_dir_all(&run_dir).map_err(|error| error.to_string())?;
    let run_edn = run_dir.join("run.edn");
    let trajectory_edn = run_dir.join("trajectory.edn");
    let report_md = run_dir.join("report.md");
    let manifest = EvalRunManifest {
        schema_version: run.schema_version,
        run_id: run.run_id.clone(),
        case: run.case.clone(),
        thread_id: run.thread_id.clone(),
        external_thread_id: run.external_thread_id.clone(),
        turn_id: run.turn_id.clone(),
        route: run.route.clone(),
        prompt: run.prompt.clone(),
        started_at: run.started_at,
        completed_at: run.completed_at,
        status: run.status.clone(),
        response: run.response.clone(),
        raw_error: run.raw_error.clone(),
        turn_policy: run.turn_policy.clone(),
        policy_violations: run.policy_violations.clone(),
        versions: run.versions.clone(),
        usage: run.usage.clone(),
        scores: scores.clone(),
        trajectory_file: "trajectory.edn".into(),
    };
    let trajectory = EvalTrajectoryFile {
        schema_version: run.schema_version,
        run_id: run.run_id.clone(),
        events: run.events.clone(),
    };
    atomic_write(&run_edn, &crate::strict_edn::to_vec(&manifest)?)?;
    atomic_write(&trajectory_edn, &crate::strict_edn::to_vec(&trajectory)?)?;
    atomic_write(&report_md, render_report(&run, &scores).as_bytes())?;
    Ok(EvalRunFiles {
        run_dir,
        run_edn,
        trajectory_edn,
        report_md,
    })
}

pub fn persist_run_with_exporter(
    root: &Path,
    run: &EvalRun,
    exporter: &dyn EvalTelemetryExporter,
) -> Result<EvalRunFiles, String> {
    let files = persist_run(root, run)?;
    exporter.export(run, &score_run(run))?;
    Ok(files)
}

pub fn read_run(run_dir: &Path) -> Result<EvalRun, String> {
    let manifest = crate::strict_edn::from_slice::<EvalRunManifest>(
        &fs::read(run_dir.join("run.edn")).map_err(|error| error.to_string())?,
    )?;
    let trajectory = crate::strict_edn::from_slice::<EvalTrajectoryFile>(
        &fs::read(run_dir.join(&manifest.trajectory_file)).map_err(|error| error.to_string())?,
    )?;
    if manifest.run_id != trajectory.run_id {
        return Err("Eval manifest and trajectory run ids differ.".into());
    }
    Ok(EvalRun {
        schema_version: manifest.schema_version,
        run_id: manifest.run_id,
        case: manifest.case,
        thread_id: manifest.thread_id,
        external_thread_id: manifest.external_thread_id,
        turn_id: manifest.turn_id,
        route: manifest.route,
        prompt: manifest.prompt,
        started_at: manifest.started_at,
        completed_at: manifest.completed_at,
        status: manifest.status,
        response: manifest.response,
        raw_error: manifest.raw_error,
        turn_policy: manifest.turn_policy,
        policy_violations: manifest.policy_violations,
        events: trajectory.events,
        versions: manifest.versions,
        usage: manifest.usage,
    })
}

pub fn compare_runs(baseline: &EvalRun, challenger: &EvalRun) -> Result<String, String> {
    if baseline.case.case_id != challenger.case.case_id
        || baseline.case.starting_input_digest != challenger.case.starting_input_digest
    {
        return Err("Paired eval runs must use the same case and starting input.".into());
    }
    let differences = [
        (
            "provider",
            baseline.route.provider != challenger.route.provider,
        ),
        ("model", baseline.route.model != challenger.route.model),
        ("effort", baseline.route.effort != challenger.route.effort),
        (
            "prompt-version",
            baseline.route.prompt_version != challenger.route.prompt_version,
        ),
    ]
    .into_iter()
    .filter_map(|(name, changed)| changed.then_some(name))
    .collect::<Vec<_>>();
    if differences.len() != 1 {
        return Err("Paired eval runs must change exactly one route variable.".into());
    }
    let difference = differences[0];
    let (baseline_value, challenger_value) = match difference {
        "provider" => (
            baseline.route.provider.as_str(),
            challenger.route.provider.as_str(),
        ),
        "model" => (
            baseline.route.model.as_deref().unwrap_or("unknown"),
            challenger.route.model.as_deref().unwrap_or("unknown"),
        ),
        "effort" => (
            baseline.route.effort.as_deref().unwrap_or("unknown"),
            challenger.route.effort.as_deref().unwrap_or("unknown"),
        ),
        "prompt-version" => (
            baseline.route.prompt_version.as_str(),
            challenger.route.prompt_version.as_str(),
        ),
        _ => unreachable!(),
    };
    let left = score_run(baseline);
    let right = score_run(challenger);
    Ok(format!(
        "# LLM eval comparison: {}\n\n| Metric | Baseline | Challenger |\n| --- | ---: | ---: |\n| Changed variable | {} | {} |\n| Completion | {} | {} |\n| First build green | {} | {} |\n| Red-to-green repair | {} | {} |\n| Unnecessary versions | {} | {} |\n| Tool calls | {} | {} |\n| Repeated adjacent tools | {} | {} |\n| Policy violations | {} | {} |\n| Latency ms | {} | {} |\n| Tokens | {} | {} |\n| Cost USD | {} | {} |\n",
        baseline.case.case_id,
        format_args!("{difference}: {baseline_value}"),
        format_args!("{difference}: {challenger_value}"),
        yes_no(left.completed),
        yes_no(right.completed),
        yes_no(left.first_build_green),
        yes_no(right.first_build_green),
        yes_no(left.red_to_green_repair),
        yes_no(right.red_to_green_repair),
        left.unnecessary_versions,
        right.unnecessary_versions,
        left.tool_calls,
        right.tool_calls,
        left.repeated_adjacent_tools,
        right.repeated_adjacent_tools,
        left.policy_violations,
        right.policy_violations,
        left.duration_ms,
        right.duration_ms,
        display_optional(left.total_tokens),
        display_optional(right.total_tokens),
        display_cost(left.estimated_cost_usd),
        display_cost(right.estimated_cost_usd),
    ))
}

pub fn version_outcomes_for_window(
    conn: &Connection,
    thread_id: &str,
    starting_version_id: Option<&str>,
    started_at: i64,
    completed_at: i64,
) -> Result<Vec<EvalVersionOutcome>, String> {
    let mut statement = conn
        .prepare(
            "SELECT id, version_input_digest, status, content, timestamp, structural_verification
             FROM messages
             WHERE thread_id = ?1 AND role = 'assistant' AND output IS NOT NULL
               AND deleted_at IS NULL AND timestamp >= ?2 AND timestamp <= ?3
             ORDER BY timestamp ASC, id ASC",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![thread_id, started_at, completed_at], |row| {
            let status: String = row.get(2)?;
            let structural_verification: Option<String> = row.get(5)?;
            let verification_passed = structural_verification
                .as_deref()
                .and_then(|encoded| serde_json::from_str::<Value>(encoded).ok())
                .and_then(|value| value.get("passed").and_then(Value::as_bool))
                .or_else(|| (status == "error").then_some(false));
            Ok(EvalVersionOutcome {
                version_id: row.get(0)?,
                input_digest: row.get(1)?,
                verification_passed,
                raw_error: (status == "error" || verification_passed == Some(false))
                    .then(|| row.get(3))
                    .transpose()?,
                status,
                created_at: row.get(4)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string())
        .map(|versions| {
            versions
                .into_iter()
                .filter(|version| Some(version.version_id.as_str()) != starting_version_id)
                .collect()
        })
}

pub fn latest_version_identity(
    conn: &Connection,
    thread_id: &str,
) -> Result<Option<(String, Option<String>)>, String> {
    conn.query_row(
        "SELECT id, version_input_digest
         FROM messages
         WHERE thread_id = ?1 AND role = 'assistant' AND output IS NOT NULL
           AND deleted_at IS NULL AND status != 'discarded'
         ORDER BY timestamp DESC, rowid DESC
         LIMIT 1",
        [thread_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .map_err(|error| error.to_string())
}

pub fn case_id(prompt: &str, starting_input_digest: Option<&str>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prompt.trim().as_bytes());
    hasher.update(b"\0");
    hasher.update(starting_input_digest.unwrap_or("none").as_bytes());
    format!("case-{:x}", hasher.finalize())
}

pub fn build_agy_eval_run(
    seed: EvalRunSeed,
    result: crate::services::agy_provider::AgyTurnResult,
    versions: Vec<EvalVersionOutcome>,
) -> EvalRun {
    let status = match result.status.as_str() {
        "SUCCESS" => "success",
        "CANCELED" | "INTERRUPTED" => "interrupted",
        _ => "error",
    }
    .to_string();
    build_eval_run(
        seed,
        result.turn_id,
        result.started_at,
        result.completed_at,
        status,
        (!result.response.trim().is_empty()).then_some(result.response),
        result.error,
        result.eval_events,
        versions,
    )
}

pub fn build_failed_eval_run(
    seed: EvalRunSeed,
    turn_id: impl Into<String>,
    started_at: i64,
    completed_at: i64,
    raw_error: impl Into<String>,
    versions: Vec<EvalVersionOutcome>,
) -> EvalRun {
    build_eval_run(
        seed,
        turn_id.into(),
        started_at,
        completed_at,
        "error".into(),
        None,
        Some(raw_error.into()),
        Vec::new(),
        versions,
    )
}

pub fn build_codex_eval_run(
    seed: EvalRunSeed,
    turn_id: impl Into<String>,
    started_at: i64,
    completed_at: i64,
    status: impl Into<String>,
    response: Option<String>,
    raw_error: Option<String>,
    events: Vec<EvalEvent>,
    versions: Vec<EvalVersionOutcome>,
) -> EvalRun {
    build_eval_run(
        seed,
        turn_id.into(),
        started_at,
        completed_at,
        status.into(),
        response,
        raw_error,
        events,
        versions,
    )
}

pub fn build_api_eval_run(
    seed: EvalRunSeed,
    turn_id: impl Into<String>,
    started_at: i64,
    completed_at: i64,
    status: impl Into<String>,
    response: Option<String>,
    raw_error: Option<String>,
    events: Vec<EvalEvent>,
    versions: Vec<EvalVersionOutcome>,
    usage: Option<EvalUsage>,
) -> EvalRun {
    let mut run = build_eval_run(
        seed,
        turn_id.into(),
        started_at,
        completed_at,
        status.into(),
        response,
        raw_error,
        events,
        versions,
    );
    run.usage = usage;
    run
}

#[allow(clippy::too_many_arguments)]
fn build_eval_run(
    seed: EvalRunSeed,
    turn_id: String,
    started_at: i64,
    completed_at: i64,
    status: String,
    response: Option<String>,
    raw_error: Option<String>,
    mut events: Vec<EvalEvent>,
    versions: Vec<EvalVersionOutcome>,
) -> EvalRun {
    if let Some(route) = seed
        .jev_route
        .as_ref()
        .filter(|_| seed.provider != "managed-mcp")
    {
        let names = events
            .iter()
            .filter_map(|event| event.name.as_deref())
            .collect::<HashSet<_>>();
        let mut initial = Vec::new();
        if !names.contains("request") {
            initial.push(EvalEvent {
                sequence: 0,
                step_index: None,
                kind: EvalEventKind::System,
                state: "admitted".into(),
                name: Some("request".into()),
                summary: Some("Application-owned provider request admitted".into()),
                input: Some(EvalPayload::new(serde_json::json!({
                    "promptSha256": format!("sha256:{:x}", Sha256::digest(seed.prompt.as_bytes())),
                    "promptChars": seed.prompt.chars().count(),
                    "threadId": seed.thread_id.clone(),
                    "externalThreadId": seed.external_thread_id.clone(),
                }))),
                output: None,
                error: None,
                occurred_at: started_at,
            });
        }
        if !names.contains("jev") && !names.contains("jev.route") {
            initial.push(EvalEvent {
                sequence: 0,
                step_index: None,
                kind: EvalEventKind::System,
                state: "accepted".into(),
                name: Some("jev".into()),
                summary: Some("Jev accepted typed route for provider request".into()),
                input: Some(EvalPayload::new(serde_json::json!({
                    "policyVersion": route.policy_version.clone(),
                    "promptTruncated": route.current_prompt_truncated,
                    "contextTruncated": route.context_truncated,
                    "classifierModel": route.classifier_model.clone(),
                    "classifierInputTokens": route.classifier_input_tokens,
                    "classifierOutputTokens": route.classifier_output_tokens,
                    "classifierLatencyMs": route.classifier_latency_ms,
                }))),
                output: Some(EvalPayload::new(serde_json::json!({
                    "intent": seed.turn_intent.as_str(),
                    "intentConfidence": route.intent_confidence,
                    "intentProbabilities": route.intent_probabilities,
                    "answerRequested": route.answer_requested,
                    "answerFirst": route.answer_first,
                    "model": route.model_ceiling,
                    "modelConfidence": route.model_confidence,
                    "modelProbabilities": route.model_probabilities,
                    "modelReason": route.model_reason.clone(),
                }))),
                error: None,
                occurred_at: started_at,
            });
        }
        if !names.contains("delivery")
            && !names.contains("provider.dispatch")
            && !names.contains("api.provider_dispatch")
        {
            initial.push(EvalEvent {
                sequence: 0,
                step_index: None,
                kind: EvalEventKind::System,
                state: "attempted".into(),
                name: Some("delivery".into()),
                summary: Some("Accepted route handed to owning provider adapter".into()),
                input: Some(EvalPayload::new(serde_json::json!({
                    "provider": seed.provider.clone(),
                    "model": seed.model.clone(),
                }))),
                output: None,
                error: None,
                occurred_at: started_at,
            });
        }
        initial.append(&mut events);
        initial.sort_by_key(|event| match event.name.as_deref() {
            Some("request") => 0,
            Some("jev" | "jev.route") => 1,
            Some("delivery" | "provider.dispatch" | "api.provider_dispatch") => 2,
            _ => 3,
        });
        events = initial;
    }
    for (sequence, event) in events.iter_mut().enumerate() {
        event.sequence = sequence as u64;
    }
    let case = EvalCase {
        case_id: case_id(&seed.prompt, seed.starting_input_digest.as_deref()),
        objective: seed.prompt.clone(),
        acceptance_criteria: vec![
            "provider reaches terminal success".into(),
            "latest changed version verifies green when the turn authors versions".into(),
        ],
        starting_version_id: seed.starting_version_id,
        starting_input_digest: seed.starting_input_digest,
        expected_red_rounds: seed.expected_red_rounds,
    };
    let mut run = EvalRun {
        schema_version: EVAL_SCHEMA_VERSION,
        run_id: seed.run_id,
        case,
        thread_id: seed.thread_id,
        external_thread_id: seed.external_thread_id,
        turn_id,
        route: EvalRoute {
            provider: seed.provider,
            model: seed.model,
            effort: seed.effort,
            prompt_version: seed.prompt_version,
            jev: seed.jev_route,
        },
        prompt: seed.prompt,
        started_at,
        completed_at,
        status,
        response,
        raw_error,
        turn_policy: Some(EvalTurnPolicy::from_policy(
            crate::provider_turn::ProviderTurnPolicy::routed(
                seed.turn_intent,
                seed.answer_first_required,
            ),
        )),
        policy_violations: Vec::new(),
        events,
        versions,
        usage: None,
    };
    run.policy_violations = evaluate_policy_violations(&run);
    run
}

/// Decode one Agy transport event into provider-neutral eval evidence. Public
/// dialogue projection may suppress payloads; this path retains a bounded,
/// recursively redacted copy for offline evaluation.
pub fn project_agy_eval_event(value: &Value, occurred_at: i64) -> Option<EvalEvent> {
    match value.get("event").and_then(Value::as_str) {
        Some("step_update") => {
            let update = value.get("step_update")?.as_object()?;
            let step_index = update.get("step_index").and_then(Value::as_i64);
            let state = update
                .get("state")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_ascii_lowercase();
            match update.get("step_type").and_then(Value::as_str) {
                Some("agent_response") => {
                    let text = update.get("text_delta").and_then(Value::as_str)?;
                    (!text.is_empty()).then(|| EvalEvent {
                        sequence: 0,
                        step_index,
                        kind: EvalEventKind::Assistant,
                        state,
                        name: None,
                        summary: Some(text.to_string()),
                        input: None,
                        output: None,
                        error: None,
                        occurred_at,
                    })
                }
                Some("tool") => {
                    let tool_info = update.get("tool_info").and_then(Value::as_object);
                    let input = tool_info
                        .and_then(|info| {
                            info.get("parameters")
                                .or_else(|| info.get("params"))
                                .or_else(|| info.get("input"))
                                .or_else(|| info.get("arguments"))
                                .or_else(|| info.get("args"))
                        })
                        .cloned()
                        .or_else(|| {
                            update
                                .get("parameters")
                                .or_else(|| update.get("params"))
                                .or_else(|| update.get("input"))
                                .cloned()
                        });
                    let output = tool_info
                        .and_then(|info| info.get("output").or_else(|| info.get("result")))
                        .cloned()
                        .or_else(|| update.get("output").cloned());
                    let nested = input.as_ref().and_then(find_provider_tool);
                    let wrapper = update
                        .get("tool_name")
                        .or_else(|| tool_info.and_then(|i| i.get("name")))
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    let name = nested
                        .map(|(server, tool)| format!("{server}/{tool}"))
                        .unwrap_or_else(|| wrapper.to_string());
                    let summary = {
                        let action = tool_public_text(update, "toolAction");
                        let sum = tool_public_text(update, "toolSummary");
                        if let Some(action) = action.filter(|a| !is_generic_tool_action(a)) {
                            format!("WORKING · {action}")
                        } else if let Some(sum) = sum.filter(|s| !is_generic_tool_action(s)) {
                            format!("WORKING · {sum}")
                        } else if let Some(detail) = format_tool_call_details(&name, input.as_ref())
                        {
                            detail
                        } else {
                            format!("USING TOOL · {name}")
                        }
                    };
                    Some(EvalEvent {
                        sequence: 0,
                        step_index,
                        kind: EvalEventKind::Tool,
                        state,
                        name: Some(name),
                        summary: Some(summary),
                        input: input.map(EvalPayload::new),
                        output: output.map(EvalPayload::new),
                        error: tool_info
                            .and_then(|info| info.get("error"))
                            .or_else(|| update.get("error"))
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        occurred_at,
                    })
                }
                _ => None,
            }
        }
        Some("result") => {
            let result = value.get("result")?.as_object()?;
            let status = result
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("error")
                .to_ascii_lowercase();
            Some(EvalEvent {
                sequence: 0,
                step_index: None,
                kind: EvalEventKind::Result,
                state: status,
                name: None,
                summary: result
                    .get("response")
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                    .map(str::to_string),
                input: None,
                output: Some(EvalPayload::new(Value::Object(result.clone()))),
                error: result
                    .get("error")
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                    .map(str::to_string),
                occurred_at,
            })
        }
        _ => None,
    }
}

pub fn is_generic_tool_action(text: &str) -> bool {
    let unquoted = text.trim_matches('"').trim_matches('\'');
    let lower = unquoted.trim().to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "calling mcp tool"
            | "call mcp tool"
            | "calling tool"
            | "call tool"
            | "using tool"
            | "execute tool"
            | "executing tool"
            | "running tool"
            | "run tool"
            | "viewing file"
            | "replacing file content"
            | "running command"
    )
}

pub fn unpack_arguments(params: &Value) -> Value {
    if let Value::Object(obj) = params {
        if let Some(args) = obj
            .get("Arguments")
            .or_else(|| obj.get("arguments"))
            .or_else(|| obj.get("args"))
        {
            if let Value::String(s) = args {
                let trimmed = s.trim();
                if (trimmed.starts_with('{') && trimmed.ends_with('}'))
                    || (trimmed.starts_with('[') && trimmed.ends_with(']'))
                {
                    if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
                        return parsed;
                    }
                }
            } else if args.is_object() {
                return args.clone();
            }
        }
    } else if let Value::String(s) = params {
        let trimmed = s.trim();
        if (trimmed.starts_with('{') && trimmed.ends_with('}'))
            || (trimmed.starts_with('[') && trimmed.ends_with(']'))
        {
            if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
                return unpack_arguments(&parsed);
            }
        }
    }
    params.clone()
}

pub fn format_tool_call_details(tool_name: &str, params: Option<&Value>) -> Option<String> {
    let params_val = params.map(unpack_arguments);
    let params_obj = params_val.as_ref().and_then(Value::as_object);

    match tool_name {
        "run_command" => {
            if let Some(cmd) = params_obj
                .and_then(|p| {
                    p.get("CommandLine")
                        .or_else(|| p.get("command"))
                        .or_else(|| p.get("cmd"))
                })
                .and_then(Value::as_str)
                .map(|s| s.trim_matches('"').trim_matches('\'').trim())
                .filter(|s| !s.is_empty())
            {
                return Some(format!("RUNNING · {cmd}"));
            }
        }
        "view_file" => {
            if let Some(path) = params_obj
                .and_then(|p| {
                    p.get("AbsolutePath")
                        .or_else(|| p.get("path"))
                        .or_else(|| p.get("file"))
                })
                .and_then(Value::as_str)
                .map(|s| s.trim_matches('"').trim_matches('\'').trim())
                .filter(|s| !s.is_empty())
            {
                let filename = std::path::Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(path);
                let start = params_obj
                    .and_then(|p| p.get("StartLine"))
                    .and_then(Value::as_i64);
                let end = params_obj
                    .and_then(|p| p.get("EndLine"))
                    .and_then(Value::as_i64);
                if let (Some(s), Some(e)) = (start, end) {
                    return Some(format!("VIEWING · {filename}:{s}-{e}"));
                }
                return Some(format!("VIEWING · {filename}"));
            }
        }
        "replace_file_content" => {
            if let Some(path) = params_obj
                .and_then(|p| {
                    p.get("TargetFile")
                        .or_else(|| p.get("path"))
                        .or_else(|| p.get("file"))
                })
                .and_then(Value::as_str)
                .map(|s| s.trim_matches('"').trim_matches('\'').trim())
                .filter(|s| !s.is_empty())
            {
                let filename = std::path::Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(path);
                let start = params_obj
                    .and_then(|p| p.get("StartLine"))
                    .and_then(Value::as_i64);
                let end = params_obj
                    .and_then(|p| p.get("EndLine"))
                    .and_then(Value::as_i64);
                if let (Some(s), Some(e)) = (start, end) {
                    return Some(format!("EDITING · {filename} (lines {s}-{e})"));
                }
                return Some(format!("EDITING · {filename}"));
            }
        }
        "write_to_file" => {
            if let Some(path) = params_obj
                .and_then(|p| {
                    p.get("TargetFile")
                        .or_else(|| p.get("path"))
                        .or_else(|| p.get("file"))
                })
                .and_then(Value::as_str)
                .map(|s| s.trim_matches('"').trim_matches('\'').trim())
                .filter(|s| !s.is_empty())
            {
                let filename = std::path::Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(path);
                return Some(format!("WRITING · {filename}"));
            }
        }
        "find_by_name" => {
            if let Some(pattern) = params_obj
                .and_then(|p| p.get("Pattern").or_else(|| p.get("pattern")))
                .and_then(Value::as_str)
                .map(|s| s.trim_matches('"').trim_matches('\'').trim())
                .filter(|s| !s.is_empty())
            {
                return Some(format!("SEARCHING · find_by_name (Pattern: {pattern})"));
            }
        }
        "grep_search" => {
            if let Some(query) = params_obj
                .and_then(|p| p.get("Query").or_else(|| p.get("query")))
                .and_then(Value::as_str)
                .map(|s| s.trim_matches('"').trim_matches('\'').trim())
                .filter(|s| !s.is_empty())
            {
                return Some(format!("SEARCHING · grep_search (Query: {query})"));
            }
        }
        _ if tool_name.ends_with("/target_macro_get") || tool_name == "target_macro_get" => {
            let start = params_obj
                .and_then(|p| p.get("startLine").or_else(|| p.get("start_line")))
                .and_then(Value::as_i64);
            let end = params_obj
                .and_then(|p| p.get("endLine").or_else(|| p.get("end_line")))
                .and_then(Value::as_i64);
            if let (Some(s), Some(e)) = (start, end) {
                return Some(format!("USING TOOL · {tool_name} (lines {s}-{e})"));
            }
        }
        _ if tool_name.ends_with("/macro_buffer_replace_range")
            || tool_name == "macro_buffer_replace_range" =>
        {
            let start = params_obj
                .and_then(|p| p.get("startLine").or_else(|| p.get("start_line")))
                .and_then(Value::as_i64);
            let end = params_obj
                .and_then(|p| p.get("endLine").or_else(|| p.get("end_line")))
                .and_then(Value::as_i64);
            if let (Some(s), Some(e)) = (start, end) {
                return Some(format!("USING TOOL · {tool_name} (lines {s}-{e})"));
            }
        }
        _ => {}
    }
    None
}

fn find_provider_tool(value: &Value) -> Option<(String, String)> {
    match value {
        Value::Object(object) => {
            let server = object
                .get("ServerName")
                .or_else(|| object.get("serverName"))
                .and_then(Value::as_str)
                .map(|s| s.trim_matches('"').trim_matches('\'').trim())
                .filter(|s| !s.is_empty());
            let tool = object
                .get("ToolName")
                .or_else(|| object.get("toolName"))
                .and_then(Value::as_str)
                .map(|s| s.trim_matches('"').trim_matches('\'').trim())
                .filter(|s| !s.is_empty());
            match (server, tool) {
                (Some(server), Some(tool)) => Some((server.to_string(), tool.to_string())),
                _ => {
                    for key in [
                        "parameters",
                        "params",
                        "input",
                        "toolInput",
                        "tool_input",
                        "arguments",
                        "args",
                    ] {
                        if let Some(val) = object.get(key).and_then(find_provider_tool) {
                            return Some(val);
                        }
                    }
                    object.values().find_map(find_provider_tool)
                }
            }
        }
        Value::Array(values) => values.iter().find_map(find_provider_tool),
        Value::String(encoded) => serde_json::from_str::<Value>(encoded)
            .ok()
            .as_ref()
            .and_then(find_provider_tool),
        _ => None,
    }
}

fn tool_public_text(
    update: &serde_json::Map<String, Value>,
    requested_name: &str,
) -> Option<String> {
    fn find(value: &Value, requested_name: &str) -> Option<String> {
        match value {
            Value::Object(object) => object
                .get(requested_name)
                .and_then(Value::as_str)
                .map(|s| s.trim_matches('"').trim_matches('\'').trim().to_string())
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    for key in [
                        "parameters",
                        "params",
                        "input",
                        "toolInput",
                        "tool_input",
                        "arguments",
                        "args",
                    ] {
                        if let Some(val) = object.get(key).and_then(|v| find(v, requested_name)) {
                            return Some(val);
                        }
                    }
                    object
                        .values()
                        .find_map(|value| find(value, requested_name))
                }),
            Value::Array(values) => values.iter().find_map(|value| find(value, requested_name)),
            Value::String(encoded) => serde_json::from_str::<Value>(encoded)
                .ok()
                .as_ref()
                .and_then(|value| find(value, requested_name)),
            _ => None,
        }
    }
    update
        .values()
        .find_map(|value| find(value, requested_name))
}

fn render_report(run: &EvalRun, scores: &EvalScores) -> String {
    let (intent, execution_mode) = run
        .turn_policy
        .as_ref()
        .map(|policy| (policy.intent.as_str(), policy.execution_mode.as_str()))
        .unwrap_or(("unknown", "unknown"));
    let mut report = format!(
        "# LLM eval: {}\n\n- Run: `{}`\n- Provider: `{}`\n- Model: `{}`\n- Intent: `{}`\n- Execution mode: `{}`\n- Status: `{}`\n\n| Metric | Value |\n| --- | ---: |\n| Completed | {} |\n| Terminal success | {} |\n| Tool calls | {} |\n| Repeated adjacent tools | {} |\n| Policy violations | {} |\n| Versions | {} |\n| Red versions | {} |\n| Unnecessary versions | {} |\n| First build green | {} |\n| Red-to-green repair | {} |\n| Duration ms | {} |\n| Tokens | {} |\n| Cost USD | {} |\n",
        run.case.case_id,
        run.run_id,
        run.route.provider,
        run.route.model.as_deref().unwrap_or("unknown"),
        intent,
        execution_mode,
        run.status,
        yes_no(scores.completed),
        yes_no(scores.terminal_success),
        scores.tool_calls,
        scores.repeated_adjacent_tools,
        scores.policy_violations,
        scores.version_count,
        scores.red_versions,
        scores.unnecessary_versions,
        yes_no(scores.first_build_green),
        yes_no(scores.red_to_green_repair),
        scores.duration_ms,
        display_optional(scores.total_tokens),
        display_cost(scores.estimated_cost_usd),
    );
    if let Some(jev) = &run.route.jev {
        report.push_str(&format!(
            "\nProvider-native tool coverage: `{}`\n",
            report_cell(&jev.native_tool_coverage)
        ));
    }
    report.push_str("\n## Event timeline\n\n| # | Event | Kind | State | Time (Unix s) | Summary | Error |\n| ---: | --- | --- | --- | ---: | --- | --- |\n");
    for event in &run.events {
        let kind = format!("{:?}", event.kind).to_ascii_lowercase();
        report.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            event.sequence,
            report_cell(event.name.as_deref().unwrap_or("unknown")),
            kind,
            report_cell(&event.state),
            event.occurred_at,
            report_cell(event.summary.as_deref().unwrap_or("")),
            report_cell(event.error.as_deref().unwrap_or("")),
        ));
    }
    report.push_str("\nFull redacted event payloads: `trajectory.edn`.\n");
    report
}

fn report_cell(value: &str) -> String {
    sanitize_text(value)
        .replace('|', "\\|")
        .replace(['\r', '\n'], " ")
}

fn validate_run(run: &EvalRun) -> Result<(), String> {
    if run.schema_version != EVAL_SCHEMA_VERSION {
        return Err(format!(
            "Unsupported eval schema version {}.",
            run.schema_version
        ));
    }
    for (label, value) in [
        ("run id", run.run_id.as_str()),
        ("case id", run.case.case_id.as_str()),
        ("thread id", run.thread_id.as_str()),
    ] {
        if value.is_empty()
            || !value
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "-_.:".contains(character))
        {
            return Err(format!(
                "Eval {label} contains unsafe path or identity characters."
            ));
        }
    }
    let missing_provider_turn_failure = run.turn_id.is_empty();
    if !missing_provider_turn_failure {
        let value = run.turn_id.as_str();
        if value.is_empty()
            || !value
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "-_.:".contains(character))
        {
            return Err("Eval turn id contains unsafe path or identity characters.".into());
        }
    } else if run.route.provider == "managed-mcp" && !run.external_thread_id.trim().is_empty() {
        let terminal_status_valid = match run.status.as_str() {
            "running" => run.response.is_none() && run.raw_error.is_none(),
            "success" => run.response.is_some() && run.raw_error.is_none(),
            "error" | "failed_pre_dispatch" => run.raw_error.is_some(),
            _ => false,
        };
        if !terminal_status_valid
            || run
                .events
                .iter()
                .any(|event| event.kind == EvalEventKind::Assistant)
            || (run.status == "failed_pre_dispatch"
                && (run
                    .events
                    .iter()
                    .any(|event| event.kind != EvalEventKind::System)
                    || !run.versions.is_empty()))
        {
            return Err("Managed MCP run without a native provider turn ID has invalid status or invented provider events.".into());
        }
    } else if run.route.prompt_version == "api-generation-v1" && run.external_thread_id.is_empty() {
        let status_valid = match run.status.as_str() {
            "success" => run.raw_error.is_none(),
            "error" | "failed_pre_dispatch" => run.raw_error.is_some(),
            "interrupted" => true,
            _ => false,
        };
        if !status_valid
            || run.events.iter().any(|event| {
                !matches!(event.kind, EvalEventKind::System | EvalEventKind::Assistant)
            })
            || (run.status == "failed_pre_dispatch"
                && (run
                    .events
                    .iter()
                    .any(|event| event.kind != EvalEventKind::System)
                    || !run.versions.is_empty()))
        {
            return Err("Stateless API run without a native provider turn ID has invalid status or invented provider events.".into());
        }
    } else if !matches!(run.status.as_str(), "failed_pre_dispatch" | "error")
        || run
            .events
            .iter()
            .any(|event| event.kind != EvalEventKind::System)
        || !run.versions.is_empty()
        || run.raw_error.is_none()
    {
        return Err(
            "Failure without a provider turn ID must have diagnostic and no provider/tool events or versions.".into(),
        );
    }
    if run.completed_at < run.started_at {
        return Err("Eval completion precedes start.".into());
    }
    if run
        .events
        .windows(2)
        .any(|pair| pair[0].sequence >= pair[1].sequence)
    {
        return Err("Eval event sequence must be strictly increasing.".into());
    }
    Ok(())
}

fn sanitize_run(mut run: EvalRun) -> EvalRun {
    run.prompt = sanitize_text(&run.prompt);
    run.response = run.response.map(|value| sanitize_text(&value));
    run.raw_error = run.raw_error.map(|value| sanitize_text(&value));
    run.policy_violations = run
        .policy_violations
        .into_iter()
        .map(|value| sanitize_text(&value))
        .collect();
    for event in &mut run.events {
        event.summary = event.summary.take().map(|value| sanitize_text(&value));
        event.error = event.error.take().map(|value| sanitize_text(&value));
    }
    for version in &mut run.versions {
        version.raw_error = version.raw_error.take().map(|value| sanitize_text(&value));
    }
    run
}

fn sanitize_value(value: Value, key: Option<&str>, truncated: &mut bool) -> EvalValue {
    if key.is_some_and(secret_key) {
        return EvalValue::String("[REDACTED]".into());
    }
    match value {
        Value::Null => EvalValue::Nil,
        Value::Bool(value) => EvalValue::Bool(value),
        Value::Number(value) => value
            .as_i64()
            .map(EvalValue::Integer)
            .or_else(|| {
                value
                    .as_u64()
                    .and_then(|value| i64::try_from(value).ok())
                    .map(EvalValue::Integer)
            })
            .or_else(|| value.as_f64().map(EvalValue::Float))
            .unwrap_or(EvalValue::Nil),
        Value::String(value) => {
            let sanitized = sanitize_text_unbounded(&value);
            let count = sanitized.chars().count();
            if count > MAX_PAYLOAD_STRING_CHARS {
                *truncated = true;
                EvalValue::String(
                    sanitized
                        .chars()
                        .take(MAX_PAYLOAD_STRING_CHARS.saturating_sub(1))
                        .collect::<String>()
                        + "…",
                )
            } else {
                EvalValue::String(sanitized)
            }
        }
        Value::Array(values) => EvalValue::List(
            values
                .into_iter()
                .map(|value| sanitize_value(value, None, truncated))
                .collect(),
        ),
        Value::Object(values) => EvalValue::Map(
            values
                .into_iter()
                .map(|(key, value)| EvalField {
                    value: sanitize_value(value, Some(&key), truncated),
                    key,
                })
                .collect(),
        ),
    }
}

fn secret_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    [
        "token",
        "secret",
        "password",
        "authorization",
        "cookie",
        "credential",
        "apikey",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

fn sanitize_text(value: &str) -> String {
    let value = sanitize_text_unbounded(value);
    if value.chars().count() <= MAX_TEXT_CHARS {
        value
    } else {
        value
            .chars()
            .take(MAX_TEXT_CHARS.saturating_sub(1))
            .collect::<String>()
            + "…"
    }
}

fn sanitize_text_unbounded(value: &str) -> String {
    static SECRET_ASSIGNMENT: OnceLock<Regex> = OnceLock::new();
    let regex = SECRET_ASSIGNMENT.get_or_init(|| {
        Regex::new(
            r"(?i)\b(api[_-]?key|token|authorization|password|secret|cookie|credential)\b(\s*[:=]\s*)([^\s,;]+)",
        )
        .expect("static secret regex")
    });
    let assigned = regex.replace_all(value, "$1$2[REDACTED]").into_owned();
    static BEARER: OnceLock<Regex> = OnceLock::new();
    let bearer = BEARER.get_or_init(|| {
        Regex::new(r"(?i)\b(bearer\s+)[A-Za-z0-9._~+/-]+=*").expect("static bearer regex")
    });
    static API_TOKEN: OnceLock<Regex> = OnceLock::new();
    let api_token = API_TOKEN
        .get_or_init(|| Regex::new(r"\bsk-[A-Za-z0-9_-]{16,}\b").expect("static API token regex"));
    let bearer = bearer.replace_all(&assigned, "$1[REDACTED]");
    api_token.replace_all(&bearer, "[REDACTED]").into_owned()
}

/// Remove common credential assignments and bearer/API tokens before a bounded
/// context payload is sent to the experimental classifier.
pub fn redact_sensitive_text(value: &str) -> String {
    sanitize_text_unbounded(value)
}

fn version_green(version: &EvalVersionOutcome) -> bool {
    version.status == "success" && version.verification_passed != Some(false)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("Eval path '{}' has no parent.", path.display()))?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("eval"),
        uuid::Uuid::new_v4()
    ));
    fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
    fs::rename(&temporary, path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        error.to_string()
    })
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn display_optional(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn display_cost(value: Option<f64>) -> String {
    value
        .map(|value| format!("{value:.6}"))
        .unwrap_or_else(|| "unknown".into())
}
