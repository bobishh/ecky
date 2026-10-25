//! Stable exploration-cycle guidance and bounded dynamic prompt envelopes.
//!
//! The static guidance is deliberately separate from cycle state.  Callers can
//! keep [`STATIC_GUIDANCE`] as a cacheable prefix and append the rendered
//! envelope to the user turn.

use serde::{Deserialize, Serialize};
use std::fmt::Write;

pub const STATIC_PROMPT_VERSION: &str = "exploration-cycle-v2";
pub const STATIC_GUIDANCE: &str = "EXPLORATION CONTROLLER\n\
Use four controller phases: PLAN, BUILD, VERIFY, DECIDE. These are state-machine\n\
phases, not a required number of model calls. PLAN selects one bounded next\n\
action with action, exact source version, hypothesis, bounded change scope, and\n\
expected evidence; never emit an executable plan tail. BUILD appends each source\n\
change as an immutable version before checks. No promote, commit, or finalize\n\
authoring action exists; success and failure are evidence attached to that version.\n\
VERIFY runs deterministic checks first. DECIDE follows deterministic evidence;\n\
model opinion cannot override a deterministic verification result. Use ASK or\n\
STOP when the next action needs the user or budget is exhausted.\n\
Do not create mutable authoring records beside immutable versions.\n\
Keep source, version identity, and verification evidence explicit.\n";

const DEFAULT_MAX_TOTAL_CHARS: usize = 16_000;
const DEFAULT_MAX_FIELD_CHARS: usize = 1_200;
const DEFAULT_MAX_LIST_ITEMS: usize = 8;
const DEFAULT_MAX_ITEM_CHARS: usize = 700;
const DEFAULT_MAX_RAW_EVIDENCE_CHARS: usize = 1_600;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CyclePhase {
    Plan,
    Build,
    Verify,
    Decide,
}

impl CyclePhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "PLAN",
            Self::Build => "BUILD",
            Self::Verify => "VERIFY",
            Self::Decide => "DECIDE",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EvidenceKind {
    RedVerification,
    TopologyFailure,
    ConstraintFailure,
    HighRiskReconstruction,
    Other,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EvidenceSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VerificationEvidence {
    pub kind: EvidenceKind,
    pub severity: EvidenceSeverity,
    pub code: String,
    /// Raw provider/checker detail. Renderer bounds this before prompt output.
    pub raw: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceState {
    #[serde(default)]
    pub items: Vec<VerificationEvidence>,
    #[serde(default)]
    pub consecutive_red_verifications: u32,
    #[serde(default)]
    pub deterministic_failed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CycleBudget {
    pub remaining_actions: u32,
    pub remaining_seconds: Option<u64>,
    pub remaining_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExplorationCycleContext {
    pub cycle_id: String,
    pub goal: String,
    pub acceptance_criteria: Vec<String>,
    pub hard_constraints: Vec<String>,
    #[serde(default)]
    pub soft_preferences: Vec<String>,
    pub current_version_id: String,
    pub current_version_input_digest: String,
    pub current_version_status: String,
    pub last_verification_evidence: EvidenceState,
    #[serde(default)]
    pub last_answer: Option<String>,
    pub remaining_budget: CycleBudget,
    pub current_phase: CyclePhase,
    pub required_next_output: String,
    #[serde(default = "default_prompt_version")]
    pub prompt_version: String,
}

fn default_prompt_version() -> String {
    STATIC_PROMPT_VERSION.to_string()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    None,
    Low,
    Medium,
    High,
    Xhigh,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RouteRole {
    CheapRouter,
    CapableAuthor,
    DeterministicVerifier,
    VisionVerifier,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelRoute {
    pub role: RouteRole,
    pub provider: String,
    pub model: String,
    pub effort: Option<ReasoningEffort>,
    pub prompt_version: String,
    pub latency_ms: Option<u64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub estimated_cost_usd: Option<f64>,
}

impl ModelRoute {
    pub fn deterministic_verifier(prompt_version: impl Into<String>) -> Self {
        Self {
            role: RouteRole::DeterministicVerifier,
            provider: "deterministic".to_string(),
            model: "builtin".to_string(),
            effort: None,
            prompt_version: prompt_version.into(),
            latency_ms: None,
            input_tokens: None,
            output_tokens: None,
            estimated_cost_usd: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RoutePolicy {
    pub cheap_router: ModelRoute,
    pub capable_author: ModelRoute,
    pub deterministic_verifier: ModelRoute,
}

impl RoutePolicy {
    pub fn choose_intent_router(&self) -> ModelRoute {
        self.cheap_router.clone()
    }

    pub fn deterministic_verifier_route(&self) -> ModelRoute {
        self.deterministic_verifier.clone()
    }

    pub fn author_route(&self) -> ModelRoute {
        self.capable_author.clone()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VerificationOutcome {
    Pass,
    Fail,
    Pending,
}

/// Deterministic verification is authoritative. A model/vision result is
/// deliberately not accepted as an override in either direction.
pub fn resolve_verification_outcome(deterministic: VerificationOutcome) -> VerificationOutcome {
    deterministic
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CyclePromptLimits {
    pub max_total_chars: usize,
    pub max_field_chars: usize,
    pub max_list_items: usize,
    pub max_item_chars: usize,
    pub max_raw_evidence_chars: usize,
}

impl Default for CyclePromptLimits {
    fn default() -> Self {
        Self {
            max_total_chars: DEFAULT_MAX_TOTAL_CHARS,
            max_field_chars: DEFAULT_MAX_FIELD_CHARS,
            max_list_items: DEFAULT_MAX_LIST_ITEMS,
            max_item_chars: DEFAULT_MAX_ITEM_CHARS,
            max_raw_evidence_chars: DEFAULT_MAX_RAW_EVIDENCE_CHARS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
    TotalLimitTooSmall { actual: usize, max: usize },
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TotalLimitTooSmall { actual, max } => {
                write!(f, "cycle prompt is {actual} chars, limit is {max}")
            }
        }
    }
}

impl std::error::Error for RenderError {}

pub fn render_cycle_envelope(context: &ExplorationCycleContext) -> Result<String, RenderError> {
    render_cycle_envelope_with_limits(context, CyclePromptLimits::default())
}

pub fn render_cycle_envelope_with_limits(
    context: &ExplorationCycleContext,
    limits: CyclePromptLimits,
) -> Result<String, RenderError> {
    let mut rendered = String::new();
    writeln!(rendered, "[EXPLORATION CYCLE]").expect("String write cannot fail");
    writeln!(
        rendered,
        "CYCLE ID: {}",
        clip(&context.cycle_id, limits.max_field_chars)
    )
    .expect("String write cannot fail");
    writeln!(
        rendered,
        "GOAL: {}",
        clip(&context.goal, limits.max_field_chars)
    )
    .expect("String write cannot fail");
    render_list(
        &mut rendered,
        "ACCEPTANCE CRITERIA",
        &context.acceptance_criteria,
        limits,
    );
    render_list(
        &mut rendered,
        "HARD CONSTRAINTS",
        &context.hard_constraints,
        limits,
    );
    render_list(
        &mut rendered,
        "SOFT PREFERENCES",
        &context.soft_preferences,
        limits,
    );
    writeln!(
        rendered,
        "CURRENT VERSION ID: {}",
        clip(&context.current_version_id, limits.max_field_chars)
    )
    .expect("String write cannot fail");
    writeln!(
        rendered,
        "CURRENT VERSION INPUT DIGEST: {}",
        clip(
            &context.current_version_input_digest,
            limits.max_field_chars
        )
    )
    .expect("String write cannot fail");
    writeln!(
        rendered,
        "CURRENT VERSION STATUS: {}",
        clip(&context.current_version_status, limits.max_field_chars)
    )
    .expect("String write cannot fail");
    let last_answer = context
        .last_answer
        .as_deref()
        .map(|answer| clip(answer, limits.max_field_chars))
        .unwrap_or_else(|| "none".to_string());
    writeln!(rendered, "LAST ANSWER: {last_answer}").expect("String write cannot fail");
    writeln!(rendered, "LAST VERIFICATION EVIDENCE:").expect("String write cannot fail");
    let evidence = &context.last_verification_evidence;
    if evidence.items.is_empty() {
        rendered.push_str("- none\n");
    } else {
        for item in evidence.items.iter().take(limits.max_list_items) {
            writeln!(
                rendered,
                "- [{:?}] {:?} {}: {}",
                item.severity,
                item.kind,
                clip(&item.code, limits.max_item_chars),
                clip(&item.raw, limits.max_raw_evidence_chars)
            )
            .expect("String write cannot fail");
        }
    }
    writeln!(
        rendered,
        "- consecutive red verifications: {}",
        evidence.consecutive_red_verifications
    )
    .expect("String write cannot fail");
    writeln!(
        rendered,
        "- deterministic verification failed: {}",
        evidence.deterministic_failed
    )
    .expect("String write cannot fail");
    writeln!(
        rendered,
        "REMAINING BUDGET: actions={}, seconds={}, tokens={}",
        context.remaining_budget.remaining_actions,
        context
            .remaining_budget
            .remaining_seconds
            .map_or_else(|| "unknown".to_string(), |value| value.to_string()),
        context
            .remaining_budget
            .remaining_tokens
            .map_or_else(|| "unknown".to_string(), |value| value.to_string())
    )
    .expect("String write cannot fail");
    writeln!(
        rendered,
        "CURRENT PHASE: {}",
        context.current_phase.as_str()
    )
    .expect("String write cannot fail");
    writeln!(
        rendered,
        "REQUIRED NEXT OUTPUT: {}",
        clip(&context.required_next_output, limits.max_field_chars)
    )
    .expect("String write cannot fail");
    writeln!(
        rendered,
        "PROMPT VERSION: {}",
        clip(&context.prompt_version, limits.max_field_chars)
    )
    .expect("String write cannot fail");

    if rendered.chars().count() > limits.max_total_chars {
        return Err(RenderError::TotalLimitTooSmall {
            actual: rendered.chars().count(),
            max: limits.max_total_chars,
        });
    }
    Ok(rendered)
}

fn render_list(output: &mut String, label: &str, values: &[String], limits: CyclePromptLimits) {
    writeln!(output, "{label}:").expect("String write cannot fail");
    if values.is_empty() {
        output.push_str("- none\n");
        return;
    }
    for value in values.iter().take(limits.max_list_items) {
        writeln!(output, "- {}", clip(value, limits.max_item_chars))
            .expect("String write cannot fail");
    }
}

fn clip(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let clipped: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        if max_chars == 0 {
            String::new()
        } else if max_chars == 1 {
            "…".to_string()
        } else {
            let mut output: String = clipped.chars().take(max_chars - 1).collect();
            output.push('…');
            output
        }
    } else {
        clipped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> ExplorationCycleContext {
        ExplorationCycleContext {
            cycle_id: "cycle-1".to_string(),
            goal: "repair bracket".to_string(),
            acceptance_criteria: vec!["deterministic verification passes".to_string()],
            hard_constraints: vec!["preserve mounting holes".to_string()],
            soft_preferences: vec!["keep the profile compact".to_string()],
            current_version_id: "version-7".to_string(),
            current_version_input_digest: "sha256:abc".to_string(),
            current_version_status: "working".to_string(),
            last_verification_evidence: EvidenceState::default(),
            last_answer: Some("Use the vertical mounting orientation.".to_string()),
            remaining_budget: CycleBudget {
                remaining_actions: 3,
                remaining_seconds: Some(120),
                remaining_tokens: Some(8_000),
            },
            current_phase: CyclePhase::Plan,
            required_next_output: "one bounded action".to_string(),
            prompt_version: STATIC_PROMPT_VERSION.to_string(),
        }
    }

    #[test]
    fn static_guidance_is_byte_stable_and_separate_from_cycle_state() {
        let first = STATIC_GUIDANCE.as_bytes();
        let mut changed = context();
        changed.current_phase = CyclePhase::Verify;
        changed.goal = "a different goal".to_string();

        assert_eq!(first, STATIC_GUIDANCE.as_bytes());
        let envelope = render_cycle_envelope(&changed).expect("valid envelope");
        assert!(envelope.contains("a different goal"));
        assert_eq!(first, STATIC_GUIDANCE.as_bytes());
    }

    #[test]
    fn renders_all_required_cycle_sections() {
        let rendered = render_cycle_envelope(&context()).expect("valid envelope");

        for label in [
            "GOAL:",
            "ACCEPTANCE CRITERIA:",
            "HARD CONSTRAINTS:",
            "SOFT PREFERENCES:",
            "CURRENT VERSION ID:",
            "CURRENT VERSION INPUT DIGEST:",
            "CURRENT VERSION STATUS:",
            "LAST ANSWER:",
            "LAST VERIFICATION EVIDENCE:",
            "REMAINING BUDGET:",
            "CURRENT PHASE:",
            "REQUIRED NEXT OUTPUT:",
        ] {
            assert!(rendered.contains(label), "missing {label}");
        }
        assert!(rendered.contains("Use the vertical mounting orientation."));
        assert!(rendered.contains("keep the profile compact"));
    }

    #[test]
    fn raw_evidence_is_bounded_and_total_prompt_has_hard_limit() {
        let mut value = context();
        value
            .last_verification_evidence
            .items
            .push(VerificationEvidence {
                kind: EvidenceKind::TopologyFailure,
                severity: EvidenceSeverity::Error,
                code: "topology".to_string(),
                raw: "x".repeat(10_000),
            });

        let rendered = render_cycle_envelope(&value).expect("bounded evidence");
        assert!(rendered.chars().count() <= DEFAULT_MAX_TOTAL_CHARS);
        assert!(rendered.contains('…'));

        let error = render_cycle_envelope_with_limits(
            &context(),
            CyclePromptLimits {
                max_total_chars: 8,
                ..CyclePromptLimits::default()
            },
        )
        .expect_err("required envelope cannot fit");
        assert!(matches!(error, RenderError::TotalLimitTooSmall { .. }));
    }

    #[test]
    fn routes_all_authoring_through_one_capable_baseline() {
        let route = |role, model: &str| ModelRoute {
            role,
            provider: "test".to_string(),
            model: model.to_string(),
            effort: Some(ReasoningEffort::Medium),
            prompt_version: STATIC_PROMPT_VERSION.to_string(),
            latency_ms: None,
            input_tokens: None,
            output_tokens: None,
            estimated_cost_usd: None,
        };
        let policy = RoutePolicy {
            cheap_router: route(RouteRole::CheapRouter, "small"),
            capable_author: route(RouteRole::CapableAuthor, "capable"),
            deterministic_verifier: ModelRoute::deterministic_verifier(STATIC_PROMPT_VERSION),
        };

        let baseline = policy.author_route();
        assert_eq!(baseline.model, "capable");
        assert_eq!(policy.choose_intent_router().model, "small");
        assert_eq!(
            policy.deterministic_verifier_route().provider,
            "deterministic"
        );
    }

    #[test]
    fn deterministic_verification_cannot_be_overridden() {
        assert_eq!(
            resolve_verification_outcome(VerificationOutcome::Fail),
            VerificationOutcome::Fail
        );
        assert_eq!(
            resolve_verification_outcome(VerificationOutcome::Pass),
            VerificationOutcome::Pass
        );
    }

    #[test]
    fn route_metadata_uses_camel_case_at_boundary() {
        let route = ModelRoute {
            role: RouteRole::CapableAuthor,
            provider: "openai".to_string(),
            model: "gpt".to_string(),
            effort: Some(ReasoningEffort::High),
            prompt_version: STATIC_PROMPT_VERSION.to_string(),
            latency_ms: Some(12),
            input_tokens: Some(10),
            output_tokens: Some(20),
            estimated_cost_usd: Some(0.01),
        };
        let json = serde_json::to_string(&route).expect("serialize route");
        assert!(json.contains("promptVersion"));
        assert!(json.contains("latencyMs"));
        assert!(json.contains("estimatedCostUsd"));
        assert!(!json.contains("prompt_version"));
    }
}
