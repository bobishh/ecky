//! Compact LLM context layer.
//!
//! These types are serialised into LLM prompts and MCP tool responses.
//! They intentionally do NOT derive `specta::Type` — they never cross the
//! Tauri command boundary.

use serde::Serialize;

use crate::contracts::{ManifestBounds, ModelManifest, StructuralVerificationResult, UiField};

#[cfg(test)]
use crate::contracts::ControlPrimitive;
use crate::models::{DesignOutput, ParamValue};

pub const MAX_DIGEST_PARTS: usize = 6;
pub const MAX_DIGEST_PARAMS: usize = 12;

// ── Digest types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PartDigest {
    pub part_id: String,
    pub label: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_role: Option<String>,
    /// Coarse bounding-box dimensions `[dx, dy, dz]` in mm.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coarse_size: Option<[f64; 3]>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ParamDigest {
    pub key: String,
    pub field_type: String,
    pub value: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub constraint: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AuthoringDigest {
    pub title: String,
    pub version_name: String,
    pub source_language: String,
    pub part_count: usize,
    pub parts: Vec<PartDigest>,
    pub param_count: usize,
    pub params: Vec<ParamDigest>,
    pub macro_line_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_part: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VerificationIssueDigest {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VerificationDigest {
    pub passed: bool,
    pub summary: String,
    pub issues: Vec<VerificationIssueDigest>,
    pub part_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_volume: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_area: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<[f64; 6]>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SelectedScopeDigest {
    pub part: PartDigest,
    pub controls: Vec<serde_json::Value>,
    pub annotations: Vec<serde_json::Value>,
    pub advisories: Vec<String>,
    pub params: Vec<ParamDigest>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ContextIntent {
    Authoring,
    Repair,
    SelectionEdit,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LlmContextSnapshot {
    pub intent: ContextIntent,
    pub authoring: AuthoringDigest,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification: Option<VerificationDigest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_scope: Option<SelectedScopeDigest>,
    pub snapshot_version: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContextDelta {
    pub from_version: u64,
    pub to_version: u64,
    pub changed_params: Vec<ParamDigest>,
    pub added_parts: Vec<PartDigest>,
    pub removed_part_ids: Vec<String>,
    pub new_warnings: Vec<String>,
}

// ── Builders ────────────────────────────────────────────────────────────────

fn coarse_size_from_bounds(bounds: &ManifestBounds) -> [f64; 3] {
    [
        bounds.x_max - bounds.x_min,
        bounds.y_max - bounds.y_min,
        bounds.z_max - bounds.z_min,
    ]
}

fn bbox_array(b: &ManifestBounds) -> [f64; 6] {
    [b.x_min, b.y_min, b.z_min, b.x_max, b.y_max, b.z_max]
}

pub fn compact_text(text: &str, max_chars: usize) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= max_chars {
        compact
    } else {
        let mut out = compact
            .chars()
            .take(max_chars.saturating_sub(1))
            .collect::<String>();
        out.push('…');
        out
    }
}

fn format_number(value: f64) -> String {
    if !value.is_finite() {
        return "NaN".to_string();
    }
    let rounded = (value * 100.0).round() / 100.0;
    if (rounded.fract()).abs() < f64::EPSILON {
        format!("{:.0}", rounded)
    } else {
        let mut out = format!("{:.2}", rounded);
        while out.contains('.') && out.ends_with('0') {
            out.pop();
        }
        if out.ends_with('.') {
            out.pop();
        }
        out
    }
}

fn format_param_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Number(number) => number
            .as_f64()
            .map(format_number)
            .unwrap_or_else(|| number.to_string()),
        serde_json::Value::Bool(flag) => flag.to_string(),
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::String(text) => format!("\"{}\"", compact_text(text, 32)),
        other => compact_text(&other.to_string(), 40),
    }
}

fn ui_field_type(field: &UiField) -> &'static str {
    match field {
        UiField::Range { .. } => "range",
        UiField::Number { .. } => "number",
        UiField::Select { .. } => "select",
        UiField::Checkbox { .. } => "checkbox",
        UiField::Image { .. } => "image",
    }
}

fn ui_field_key(field: &UiField) -> &str {
    match field {
        UiField::Range { key, .. }
        | UiField::Number { key, .. }
        | UiField::Select { key, .. }
        | UiField::Checkbox { key, .. }
        | UiField::Image { key, .. } => key,
    }
}

fn ui_field_constraint(field: &UiField) -> Option<serde_json::Value> {
    match field {
        UiField::Range { min, max, step, .. } | UiField::Number { min, max, step, .. } => {
            let obj = serde_json::json!({
                "min": min,
                "max": max,
                "step": step,
            });
            Some(obj)
        }
        UiField::Select { options, .. } => {
            let vals: Vec<_> = options.iter().map(|o| &o.value).collect();
            Some(serde_json::json!(vals))
        }
        UiField::Checkbox { .. } | UiField::Image { .. } => None,
    }
}

fn param_value_to_json(v: &ParamValue) -> serde_json::Value {
    match v {
        ParamValue::String(s) => serde_json::Value::String(s.clone()),
        ParamValue::Number(n) => serde_json::json!(n),
        ParamValue::Boolean(b) => serde_json::json!(b),
        ParamValue::Null => serde_json::Value::Null,
    }
}

pub fn build_authoring_digest(
    design: &DesignOutput,
    manifest: Option<&ModelManifest>,
    selected_part_id: Option<&str>,
) -> AuthoringDigest {
    let parts: Vec<PartDigest> = manifest
        .map(|m| {
            m.parts
                .iter()
                .map(|p| PartDigest {
                    part_id: p.part_id.clone(),
                    label: p.label.clone(),
                    kind: p.kind.clone(),
                    semantic_role: p.semantic_role.clone(),
                    coarse_size: p.bounds.as_ref().map(coarse_size_from_bounds),
                })
                .collect()
        })
        .unwrap_or_default();

    let params: Vec<ParamDigest> = design
        .ui_spec
        .fields
        .iter()
        .map(|field| {
            let key = ui_field_key(field).to_string();
            let value = design
                .initial_params
                .get(&key)
                .map(param_value_to_json)
                .unwrap_or(serde_json::Value::Null);
            ParamDigest {
                key,
                field_type: ui_field_type(field).to_string(),
                value,
                constraint: ui_field_constraint(field),
            }
        })
        .collect();

    AuthoringDigest {
        title: design.title.clone(),
        version_name: design.version_name.clone(),
        source_language: format!("{:?}", design.source_language),
        part_count: parts.len(),
        parts,
        param_count: params.len(),
        params,
        macro_line_count: design.macro_code.lines().count(),
        selected_part: selected_part_id.map(String::from),
    }
}

pub fn build_verification_digest(result: &StructuralVerificationResult) -> VerificationDigest {
    VerificationDigest {
        passed: result.passed,
        summary: result.summary.clone(),
        issues: result
            .issues
            .iter()
            .map(|i| VerificationIssueDigest {
                code: i.code.clone(),
                message: i.message.clone(),
                part_id: i.part_id.clone(),
            })
            .collect(),
        part_count: result.metrics.part_count,
        total_volume: result.metrics.total_volume,
        total_area: result.metrics.total_area,
        bbox: result.metrics.bbox.as_ref().map(bbox_array),
    }
}

pub fn build_selected_scope_digest(
    manifest: &ModelManifest,
    design: &DesignOutput,
    part_id: &str,
) -> Option<SelectedScopeDigest> {
    let part_binding = manifest.parts.iter().find(|p| p.part_id == part_id)?;
    let part = PartDigest {
        part_id: part_binding.part_id.clone(),
        label: part_binding.label.clone(),
        kind: part_binding.kind.clone(),
        semantic_role: part_binding.semantic_role.clone(),
        coarse_size: part_binding.bounds.as_ref().map(coarse_size_from_bounds),
    };

    let controls: Vec<serde_json::Value> = manifest
        .control_primitives
        .iter()
        .filter(|c| c.part_ids.contains(&part_id.to_string()))
        .filter_map(|c| serde_json::to_value(c).ok())
        .collect();

    let annotations: Vec<serde_json::Value> = manifest
        .measurement_annotations
        .iter()
        .filter(|a| a.target_ids.iter().any(|tid| tid == part_id))
        .filter_map(|a| serde_json::to_value(a).ok())
        .collect();

    let advisories: Vec<String> = manifest
        .advisories
        .iter()
        .filter(|a| {
            // Advisory applies to part if any of its primitives belong to this part
            a.primitive_ids.iter().any(|pid| {
                manifest
                    .control_primitives
                    .iter()
                    .any(|c| c.primitive_id == *pid && c.part_ids.contains(&part_id.to_string()))
            })
        })
        .map(|a| a.message.clone())
        .collect();

    let part_param_keys = &part_binding.parameter_keys;
    let params: Vec<ParamDigest> = design
        .ui_spec
        .fields
        .iter()
        .filter(|f| part_param_keys.contains(&ui_field_key(f).to_string()))
        .map(|field| {
            let key = ui_field_key(field).to_string();
            let value = design
                .initial_params
                .get(&key)
                .map(param_value_to_json)
                .unwrap_or(serde_json::Value::Null);
            ParamDigest {
                key,
                field_type: ui_field_type(field).to_string(),
                value,
                constraint: ui_field_constraint(field),
            }
        })
        .collect();

    Some(SelectedScopeDigest {
        part,
        controls,
        annotations,
        advisories,
        params,
    })
}

pub fn format_authoring_digest_text(digest: &AuthoringDigest) -> String {
    let mut sections = vec![format!(
        "Current working snapshot\n{} [{}] ({})",
        compact_text(&digest.title, 64),
        compact_text(&digest.version_name, 48),
        digest.source_language
    )];

    let mut field_counts = std::collections::BTreeMap::<&str, usize>::new();
    for param in &digest.params {
        *field_counts.entry(param.field_type.as_str()).or_default() += 1;
    }
    if digest.param_count > 0 {
        let breakdown = field_counts
            .into_iter()
            .map(|(kind, count)| format!("{kind}={count}"))
            .collect::<Vec<_>>()
            .join(", ");
        sections.push(if breakdown.is_empty() {
            format!("UI fields: {}", digest.param_count)
        } else {
            format!("UI fields: {} ({})", digest.param_count, breakdown)
        });

        let mut param_lines = vec![format!("Current params: {}", digest.param_count)];
        for param in digest.params.iter().take(MAX_DIGEST_PARAMS) {
            let constraint = param
                .constraint
                .as_ref()
                .map(|value| compact_text(&value.to_string(), 48))
                .filter(|value| !value.is_empty())
                .map(|value| format!(" [{}]", value))
                .unwrap_or_default();
            param_lines.push(format!(
                "- {}: {} = {}{}",
                param.key,
                param.field_type,
                format_param_value(&param.value),
                constraint
            ));
        }
        if digest.params.len() > MAX_DIGEST_PARAMS {
            param_lines.push(format!(
                "- … {} more params",
                digest.params.len() - MAX_DIGEST_PARAMS
            ));
        }
        sections.push(param_lines.join("\n"));
    }

    let mut part_lines = vec![format!("Model parts: {}", digest.part_count)];
    for part in digest.parts.iter().take(MAX_DIGEST_PARTS) {
        let kind = if part.kind.trim().is_empty() {
            String::new()
        } else {
            format!(" [{}]", compact_text(&part.kind, 18))
        };
        let role = part
            .semantic_role
            .as_ref()
            .filter(|value| !value.trim().is_empty())
            .map(|value| format!(" role={}", compact_text(value, 18)))
            .unwrap_or_default();
        let size = part.coarse_size.map(|[dx, dy, dz]| {
            format!(
                " size≈{}×{}×{} mm",
                format_number(dx),
                format_number(dy),
                format_number(dz)
            )
        });
        part_lines.push(format!(
            "- {}{}{}{}",
            compact_text(&part.label, 40),
            kind,
            role,
            size.unwrap_or_default()
        ));
    }
    if digest.parts.len() > MAX_DIGEST_PARTS {
        part_lines.push(format!(
            "- … {} more parts",
            digest.parts.len() - MAX_DIGEST_PARTS
        ));
    }
    sections.push(part_lines.join("\n"));

    if let Some(selected_part) = digest
        .selected_part
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        sections.push(format!("Selected part: {}", selected_part));
    }

    sections.push(format!("Macro lines: {}", digest.macro_line_count));
    sections.join("\n\n")
}

pub fn build_context_snapshot(
    intent: ContextIntent,
    design: &DesignOutput,
    manifest: Option<&ModelManifest>,
    verification: Option<&StructuralVerificationResult>,
    selected_part_id: Option<&str>,
    version: u64,
) -> LlmContextSnapshot {
    let authoring = build_authoring_digest(design, manifest, selected_part_id);
    let verification_digest = verification.map(build_verification_digest);
    let selected_scope = selected_part_id
        .and_then(|pid| manifest.and_then(|m| build_selected_scope_digest(m, design, pid)));

    LlmContextSnapshot {
        intent,
        authoring,
        verification: verification_digest,
        selected_scope,
        snapshot_version: version,
    }
}

pub fn build_context_delta(
    prev: &LlmContextSnapshot,
    next: &LlmContextSnapshot,
) -> Option<ContextDelta> {
    let changed_params: Vec<ParamDigest> = next
        .authoring
        .params
        .iter()
        .filter(|np| {
            prev.authoring
                .params
                .iter()
                .find(|pp| pp.key == np.key)
                .is_none_or(|pp| pp.value != np.value)
        })
        .cloned()
        .collect();

    let prev_part_ids: std::collections::HashSet<&str> = prev
        .authoring
        .parts
        .iter()
        .map(|p| p.part_id.as_str())
        .collect();
    let next_part_ids: std::collections::HashSet<&str> = next
        .authoring
        .parts
        .iter()
        .map(|p| p.part_id.as_str())
        .collect();

    let added_parts: Vec<PartDigest> = next
        .authoring
        .parts
        .iter()
        .filter(|p| !prev_part_ids.contains(p.part_id.as_str()))
        .cloned()
        .collect();
    let removed_part_ids: Vec<String> = prev_part_ids
        .difference(&next_part_ids)
        .map(|s| s.to_string())
        .collect();

    let new_warnings = Vec::new(); // TODO: populate from manifest warnings diff

    if changed_params.is_empty()
        && added_parts.is_empty()
        && removed_part_ids.is_empty()
        && new_warnings.is_empty()
    {
        return None;
    }

    Some(ContextDelta {
        from_version: prev.snapshot_version,
        to_version: next.snapshot_version,
        changed_params,
        added_parts,
        removed_part_ids,
        new_warnings,
    })
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{
        ManifestBounds, PartBinding, StructuralIssue, StructuralMetrics, VerifierStatus,
    };
    use crate::models::{
        DesignOutput, EngineKind, GeometryBackend, InteractionMode, MacroDialect, SourceLanguage,
        UiSpec,
    };
    use std::collections::BTreeMap;

    fn test_design() -> DesignOutput {
        let mut params = BTreeMap::new();
        params.insert("radius".to_string(), ParamValue::Number(30.0));
        params.insert("height".to_string(), ParamValue::Number(50.0));

        DesignOutput {
            title: "Test Dome".to_string(),
            version_name: "V1".to_string(),
            response: String::new(),
            interaction_mode: InteractionMode::Design,
            macro_dialect: MacroDialect::CadFrameworkV1,
            engine_kind: EngineKind::Freecad,
            source_language: SourceLanguage::LegacyPython,
            geometry_backend: GeometryBackend::Freecad,
            macro_code: "import FreeCAD\npart = make_dome(30)\n".to_string(),
            ui_spec: UiSpec {
                fields: vec![
                    UiField::Range {
                        key: "radius".to_string(),
                        label: "Radius".to_string(),
                        min: Some(1.0),
                        max: Some(100.0),
                        step: Some(1.0),
                        min_from: None,
                        max_from: None,
                        frozen: false,
                    },
                    UiField::Number {
                        key: "height".to_string(),
                        label: "Height".to_string(),
                        min: Some(5.0),
                        max: Some(200.0),
                        step: None,
                        min_from: None,
                        max_from: None,
                        frozen: false,
                    },
                ],
            },
            initial_params: params,
            post_processing: None,
        }
    }

    fn test_manifest() -> ModelManifest {
        ModelManifest {
            schema_version: 2,
            model_id: "model-1".to_string(),
            source_kind: crate::contracts::ModelSourceKind::Generated,
            engine_kind: EngineKind::Freecad,
            source_language: SourceLanguage::LegacyPython,
            geometry_backend: GeometryBackend::Freecad,
            document: crate::contracts::DocumentMetadata {
                document_name: "doc".to_string(),
                document_label: "Doc".to_string(),
                source_path: Some("/tmp/doc.fcstd".to_string()),
                object_count: 3,
                warnings: vec![],
            },
            parts: vec![
                PartBinding {
                    part_id: "part-1".to_string(),
                    freecad_object_name: "Part1".to_string(),
                    label: "Dome Body".to_string(),
                    kind: "solid".to_string(),
                    semantic_role: Some("body".to_string()),
                    viewer_asset_path: Some("/tmp/assets/part1.stl".to_string()),
                    viewer_node_ids: vec!["node-1".to_string()],
                    parameter_keys: vec!["radius".to_string()],
                    editable: true,
                    bounds: Some(ManifestBounds {
                        x_min: -30.0,
                        y_min: -30.0,
                        z_min: 0.0,
                        x_max: 30.0,
                        y_max: 30.0,
                        z_max: 30.0,
                    }),
                    volume: Some(56548.0),
                    area: Some(5654.0),
                },
                PartBinding {
                    part_id: "part-2".to_string(),
                    freecad_object_name: "Part2".to_string(),
                    label: "Base".to_string(),
                    kind: "solid".to_string(),
                    semantic_role: None,
                    viewer_asset_path: Some("/tmp/assets/part2.stl".to_string()),
                    viewer_node_ids: vec!["node-2".to_string()],
                    parameter_keys: vec!["height".to_string()],
                    editable: true,
                    bounds: Some(ManifestBounds {
                        x_min: -40.0,
                        y_min: -40.0,
                        z_min: -10.0,
                        x_max: 40.0,
                        y_max: 40.0,
                        z_max: 0.0,
                    }),
                    volume: Some(32000.0),
                    area: Some(9600.0),
                },
            ],
            parameter_groups: vec![],
            control_primitives: vec![ControlPrimitive {
                primitive_id: "ctrl-radius".to_string(),
                label: "Dome Radius".to_string(),
                kind: crate::contracts::ControlPrimitiveKind::Number,
                source: crate::contracts::ControlViewSource::Llm,
                part_ids: vec!["part-1".to_string()],
                bindings: vec![],
                editable: true,
                order: 0,
            }],
            control_relations: vec![],
            control_views: vec![],
            advisories: vec![],
            selection_targets: vec![],
            measurement_annotations: vec![],
            warnings: vec![],
            enrichment_state: crate::contracts::ManifestEnrichmentState {
                status: crate::contracts::EnrichmentStatus::None,
                proposals: Vec::new(),
            },
        }
    }

    fn test_verification_result(passed: bool) -> StructuralVerificationResult {
        StructuralVerificationResult {
            passed,
            summary: if passed {
                "All checks passed.".to_string()
            } else {
                "Structural verification failed: PREVIEW_STL_MISSING".to_string()
            },
            issues: if passed {
                vec![]
            } else {
                vec![StructuralIssue {
                    code: "PREVIEW_STL_MISSING".to_string(),
                    message: "Preview STL not found.".to_string(),
                    part_id: None,
                    numeric_payload: None,
                }]
            },
            metrics: StructuralMetrics {
                part_count: 2,
                preview_stl_size_bytes: Some(4096),
                total_volume: Some(88548.0),
                total_area: Some(15254.0),
                bbox: Some(ManifestBounds {
                    x_min: -40.0,
                    y_min: -40.0,
                    z_min: -10.0,
                    x_max: 40.0,
                    y_max: 40.0,
                    z_max: 30.0,
                }),
            },
            verifier_status: VerifierStatus::OkRustOnly,
            verifier_source: None,
        }
    }

    // ── AuthoringDigest ─────────────────────────────────────────────────────

    #[test]
    fn authoring_digest_excludes_paths_and_viewer_assets() {
        let design = test_design();
        let manifest = test_manifest();
        let digest = build_authoring_digest(&design, Some(&manifest), None);

        let json = serde_json::to_string(&digest).unwrap();
        assert!(
            !json.contains("/tmp/"),
            "digest must not contain filesystem paths"
        );
        assert!(
            !json.contains("viewer_asset"),
            "digest must not contain viewer_asset fields"
        );
        assert!(
            !json.contains("viewerAsset"),
            "digest must not contain viewerAsset fields"
        );
        assert!(
            !json.contains("fcstd"),
            "digest must not contain fcstd references"
        );
    }

    #[test]
    fn authoring_digest_param_count_matches_ui_fields() {
        let design = test_design();
        let digest = build_authoring_digest(&design, None, None);

        assert_eq!(digest.param_count, 2);
        assert_eq!(digest.params.len(), 2);
        assert_eq!(digest.params[0].key, "radius");
        assert_eq!(digest.params[0].field_type, "range");
        assert_eq!(digest.params[1].key, "height");
        assert_eq!(digest.params[1].field_type, "number");
    }

    #[test]
    fn authoring_digest_part_count_from_manifest() {
        let design = test_design();
        let manifest = test_manifest();
        let digest = build_authoring_digest(&design, Some(&manifest), None);

        assert_eq!(digest.part_count, 2);
        assert_eq!(digest.parts.len(), 2);
        assert_eq!(digest.parts[0].part_id, "part-1");
        assert_eq!(digest.parts[0].label, "Dome Body");
        assert!(digest.parts[0].coarse_size.is_some());
    }

    #[test]
    fn authoring_digest_without_manifest_has_no_parts() {
        let design = test_design();
        let digest = build_authoring_digest(&design, None, None);

        assert_eq!(digest.part_count, 0);
        assert!(digest.parts.is_empty());
    }

    #[test]
    fn authoring_digest_macro_line_count() {
        let design = test_design();
        let digest = build_authoring_digest(&design, None, None);

        assert_eq!(digest.macro_line_count, 2);
    }

    #[test]
    fn authoring_digest_selected_part() {
        let design = test_design();
        let digest = build_authoring_digest(&design, None, Some("part-1"));
        assert_eq!(digest.selected_part.as_deref(), Some("part-1"));

        let digest2 = build_authoring_digest(&design, None, None);
        assert!(digest2.selected_part.is_none());
    }

    #[test]
    fn authoring_digest_text_omits_backend_and_paths() {
        let design = test_design();
        let manifest = test_manifest();
        let digest = build_authoring_digest(&design, Some(&manifest), Some("part-1"));

        let text = format_authoring_digest_text(&digest);
        assert!(text.contains("Current working snapshot"));
        assert!(text.contains("Model parts: 2"));
        assert!(text.contains("Current params: 2"));
        assert!(text.contains("Selected part: part-1"));
        assert!(!text.contains("/tmp/"));
        assert!(!text.contains("Freecad"));
        assert!(!text.contains("Build123d"));
        assert!(!text.contains("EckyRust"));
    }

    // ── VerificationDigest ──────────────────────────────────────────────────

    #[test]
    fn verification_digest_from_structural_result() {
        let result = test_verification_result(false);
        let digest = build_verification_digest(&result);

        assert!(!digest.passed);
        assert_eq!(digest.issues.len(), 1);
        assert_eq!(digest.issues[0].code, "PREVIEW_STL_MISSING");
        assert_eq!(digest.part_count, 2);
        assert!(digest.total_volume.is_some());
        assert!(digest.bbox.is_some());
    }

    #[test]
    fn verification_digest_passing_has_no_issues() {
        let result = test_verification_result(true);
        let digest = build_verification_digest(&result);

        assert!(digest.passed);
        assert!(digest.issues.is_empty());
    }

    // ── SelectedScopeDigest ─────────────────────────────────────────────────

    #[test]
    fn selected_scope_filters_by_part() {
        let design = test_design();
        let manifest = test_manifest();
        let scope = build_selected_scope_digest(&manifest, &design, "part-1").unwrap();

        assert_eq!(scope.part.part_id, "part-1");
        assert_eq!(scope.controls.len(), 1); // ctrl-radius belongs to part-1
        assert_eq!(scope.params.len(), 1); // radius is bound to part-1
        assert_eq!(scope.params[0].key, "radius");
    }

    #[test]
    fn selected_scope_returns_none_for_unknown_part() {
        let design = test_design();
        let manifest = test_manifest();
        assert!(build_selected_scope_digest(&manifest, &design, "nonexistent").is_none());
    }

    // ── ContextDelta ────────────────────────────────────────────────────────

    #[test]
    fn context_delta_detects_param_change() {
        let design = test_design();
        let snap1 = build_context_snapshot(ContextIntent::Authoring, &design, None, None, None, 1);

        let mut design2 = test_design();
        design2
            .initial_params
            .insert("radius".to_string(), ParamValue::Number(45.0));
        let snap2 = build_context_snapshot(ContextIntent::Authoring, &design2, None, None, None, 2);

        let delta = build_context_delta(&snap1, &snap2).unwrap();
        assert_eq!(delta.from_version, 1);
        assert_eq!(delta.to_version, 2);
        assert_eq!(delta.changed_params.len(), 1);
        assert_eq!(delta.changed_params[0].key, "radius");
        assert!(delta.added_parts.is_empty());
        assert!(delta.removed_part_ids.is_empty());
    }

    #[test]
    fn context_delta_returns_none_when_identical() {
        let design = test_design();
        let snap = build_context_snapshot(ContextIntent::Authoring, &design, None, None, None, 1);
        assert!(build_context_delta(&snap, &snap).is_none());
    }

    #[test]
    fn context_delta_detects_added_and_removed_parts() {
        let design = test_design();
        let manifest1 = test_manifest();
        let snap1 = build_context_snapshot(
            ContextIntent::Authoring,
            &design,
            Some(&manifest1),
            None,
            None,
            1,
        );

        let mut manifest2 = test_manifest();
        manifest2.parts.remove(1); // remove "part-2"
        manifest2.parts.push(PartBinding {
            part_id: "part-3".to_string(),
            freecad_object_name: "Part3".to_string(),
            label: "Lid".to_string(),
            kind: "solid".to_string(),
            semantic_role: None,
            viewer_asset_path: None,
            viewer_node_ids: vec![],
            parameter_keys: vec![],
            editable: true,
            bounds: None,
            volume: None,
            area: None,
        });
        let snap2 = build_context_snapshot(
            ContextIntent::Authoring,
            &design,
            Some(&manifest2),
            None,
            None,
            2,
        );

        let delta = build_context_delta(&snap1, &snap2).unwrap();
        assert_eq!(delta.added_parts.len(), 1);
        assert_eq!(delta.added_parts[0].part_id, "part-3");
        assert_eq!(delta.removed_part_ids.len(), 1);
        assert_eq!(delta.removed_part_ids[0], "part-2");
    }

    // ── LlmContextSnapshot ─────────────────────────────────────────────────

    #[test]
    fn context_snapshot_repair_includes_verification() {
        let design = test_design();
        let verification = test_verification_result(false);
        let snap = build_context_snapshot(
            ContextIntent::Repair,
            &design,
            None,
            Some(&verification),
            None,
            1,
        );

        assert_eq!(snap.intent, ContextIntent::Repair);
        assert!(snap.verification.is_some());
        assert!(!snap.verification.as_ref().unwrap().passed);
    }

    #[test]
    fn context_snapshot_authoring_has_no_verification() {
        let design = test_design();
        let snap = build_context_snapshot(ContextIntent::Authoring, &design, None, None, None, 1);

        assert_eq!(snap.intent, ContextIntent::Authoring);
        assert!(snap.verification.is_none());
        assert!(snap.selected_scope.is_none());
    }
}
