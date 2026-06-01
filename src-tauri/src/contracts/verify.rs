use super::{DiagnosticContext, ManifestBounds, UsageSummary};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RenderVerification {
    /// true = model matches the prompt, no action needed
    pub passed: bool,
    /// human-readable description of what's wrong (empty when passed)
    pub issues: String,
    #[serde(default)]
    pub usage: Option<UsageSummary>,
}

// ── Structural verification ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StructuralVerificationResult {
    pub passed: bool,
    pub summary: String,
    pub issues: Vec<StructuralIssue>,
    #[serde(default)]
    pub authored_verify_checks: Vec<AuthoredVerifyCheck>,
    pub metrics: StructuralMetrics,
    pub verifier_status: VerifierStatus,
    #[serde(default)]
    pub verifier_source: Option<VerifierSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AuthoredVerifyCheckStatus {
    Passed,
    Failed,
    Error,
}

/// A resolved verify value (expected or actual). Boundary-typed so MCP agents
/// and UI chips read machine values instead of parsing the message string.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum AuthoredVerifyValue {
    Number(f64),
    Boolean(bool),
    Text(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AuthoredVerifyCheck {
    pub tag: String,
    pub status: AuthoredVerifyCheckStatus,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stable_node_id: Option<String>,
    /// Machine-readable delta: where the metric came from, the comparator, and
    /// the expected vs actual values. Lets the agent fix a red check without
    /// re-parsing `message`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparator: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<AuthoredVerifyValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual: Option<AuthoredVerifyValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic_context: Option<DiagnosticContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerifierSource {
    RustStructural,
    RustPlusBackend,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StructuralIssue {
    pub code: String,
    pub message: String,
    /// ID of the affected part, when the issue is part-specific.
    #[serde(default)]
    pub part_id: Option<String>,
    #[serde(default)]
    pub numeric_payload: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic_context: Option<DiagnosticContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StructuralMetrics {
    pub part_count: u32,
    #[serde(default)]
    pub preview_stl_size_bytes: Option<u64>,
    #[serde(default)]
    pub preview_stl_triangle_count: Option<u32>,
    #[serde(default)]
    pub preview_stl_component_count: Option<u32>,
    #[serde(default)]
    pub preview_stl_non_manifold_edge_count: Option<u32>,
    #[serde(default)]
    pub preview_stl_overhang_triangle_count: Option<u32>,
    #[serde(default)]
    pub preview_stl_overhang_ratio: Option<f64>,
    #[serde(default)]
    pub total_volume: Option<f64>,
    #[serde(default)]
    pub total_area: Option<f64>,
    #[serde(default)]
    pub bbox: Option<ManifestBounds>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerifierStatus {
    Ok,
    OkRustOnly,
    OkWithBackend,
    SkippedUnavailable,
    SkippedBackendUnavailable,
}

// ── Visual (screenshot) verification ────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VisualVerificationResult {
    pub passed: bool,
    pub summary: String,
    pub issues: Vec<VisualIssue>,
    #[serde(default)]
    pub usage: Option<UsageSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VisualIssue {
    pub category: VisualIssueCategory,
    pub description: String,
    #[serde(default)]
    pub part_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VisualIssueCategory {
    MissingPart,
    FloatingPart,
    ConnectorBroken,
    ReferenceMismatch,
    TopologyBroken,
    Other,
}

impl VisualIssueCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MissingPart => "missing_part",
            Self::FloatingPart => "floating_part",
            Self::ConnectorBroken => "connector_broken",
            Self::ReferenceMismatch => "reference_mismatch",
            Self::TopologyBroken => "topology_broken",
            Self::Other => "other",
        }
    }
}

// ── End structural verification ─────────────────────────────────────────────
