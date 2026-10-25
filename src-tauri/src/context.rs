use crate::context_envelope::{
    approx_tokens, assemble_envelope, measure_chars, project_sections, section_id,
    ContextBudgetError, ContextEnvelope, ContextInputs, ContextSection, EnvelopeStage,
    InclusionDecision, ProjectionIntent, SectionPriority, Sensitivity,
};
use crate::contracts::{
    infer_macro_dialect_from_code, ArtifactBundle, DesignOutput, EngineKind, GeometryBackend,
    InteractionMode, Message, MessageRole, ModelManifest, SourceLanguage, ThreadReference, UiSpec,
};
use crate::llm_context::{build_authoring_digest, format_authoring_digest_text};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedAuthoringContext {
    pub engine_kind: EngineKind,
    pub source_language: SourceLanguage,
    pub geometry_backend: GeometryBackend,
}

pub const THREAD_SUMMARY_MAX_CHARS: usize = 1600;
pub const SUMMARY_ITEM_MAX_CHARS: usize = 220;
pub const RECENT_DIALOGUE_MAX_MESSAGES: usize = 6;
pub const RECENT_DIALOGUE_ITEM_MAX_CHARS: usize = 260;
pub const PINNED_REFERENCES_MAX_ITEMS: usize = 4;
pub const PINNED_REFERENCE_CONTENT_MAX_CHARS: usize = 2200;
pub const PINNED_REFERENCE_SUMMARY_MAX_CHARS: usize = 200;

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

pub fn latest_output(messages: &[Message]) -> Option<DesignOutput> {
    messages
        .iter()
        .rev()
        .find(|m| m.role == MessageRole::Assistant && m.output.is_some())
        .and_then(|m| m.output.clone())
}

pub fn latest_manifest(messages: &[Message]) -> Option<ModelManifest> {
    messages
        .iter()
        .rev()
        .find(|m| m.role == MessageRole::Assistant && m.model_manifest.is_some())
        .and_then(|m| m.model_manifest.clone())
}

pub fn latest_artifact_bundle(messages: &[Message]) -> Option<ArtifactBundle> {
    messages
        .iter()
        .rev()
        .find(|m| m.role == MessageRole::Assistant && m.artifact_bundle.is_some())
        .and_then(|m| m.artifact_bundle.clone())
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LatestAssistantSnapshot {
    pub output: Option<DesignOutput>,
    pub model_manifest: Option<ModelManifest>,
    pub artifact_bundle: Option<ArtifactBundle>,
}

pub fn latest_assistant_snapshot(messages: &[Message]) -> LatestAssistantSnapshot {
    messages
        .iter()
        .rev()
        .find(|message| {
            message.role == MessageRole::Assistant
                && (message.output.is_some()
                    || message.model_manifest.is_some()
                    || message.artifact_bundle.is_some())
        })
        .map(|message| LatestAssistantSnapshot {
            output: message.output.clone(),
            model_manifest: message.model_manifest.clone(),
            artifact_bundle: message.artifact_bundle.clone(),
        })
        .unwrap_or_default()
}

pub fn build_design_digest(
    output: Option<&DesignOutput>,
    manifest: Option<&ModelManifest>,
) -> String {
    output
        .map(|design| format_authoring_digest_text(&build_authoring_digest(design, manifest, None)))
        .unwrap_or_default()
}

pub fn build_artifact_digest(bundle: Option<&ArtifactBundle>) -> String {
    let Some(bundle) = bundle else {
        return String::new();
    };

    let mut export_formats = bundle
        .export_artifacts
        .iter()
        .map(|artifact| artifact.format.trim().to_ascii_lowercase())
        .filter(|format| !format.is_empty())
        .collect::<Vec<_>>();
    export_formats.sort();
    export_formats.dedup();

    let step_export_path = bundle
        .export_artifacts
        .iter()
        .find(|artifact| artifact.format.eq_ignore_ascii_case("step"))
        .map(|artifact| artifact.path.as_str())
        .filter(|path| !path.trim().is_empty());

    [
        format!("modelId: {}", bundle.model_id),
        format!("sourceLanguage: {}", source_language_label(bundle.source_language)),
        format!(
            "geometryBackend: {}",
            geometry_backend_label(bundle.geometry_backend)
        ),
        format!("hasModelStl: {}", !bundle.model_stl_path.trim().is_empty()),
        format!("viewerAssetCount: {}", bundle.viewer_assets.len()),
        format!("edgeTargetCount: {}", bundle.edge_targets.len()),
        format!("faceTargetCount: {}", bundle.face_targets.len()),
        format!("exportFormatCount: {}", bundle.export_artifacts.len()),
        format!(
            "exportFormats: {}",
            if export_formats.is_empty() {
                "[none]".to_string()
            } else {
                export_formats.join(", ")
            }
        ),
        format!("hasStepExport: {}", step_export_path.is_some()),
        format!(
            "stepExportPath: {}",
            step_export_path.unwrap_or("[none]")
        ),
        "STEP rule: only offer STEP for this exact artifact when hasStepExport is true; do not infer STEP from backend or capability.".to_string(),
    ]
    .join("\n")
}

pub fn build_thread_summary(title: &str, messages: &[Message]) -> String {
    let mut sections: Vec<String> = Vec::new();

    if !title.trim().is_empty() {
        sections.push(format!(
            "Thread: {}",
            compact_text(title, SUMMARY_ITEM_MAX_CHARS)
        ));
    }

    if let Some(output) = latest_output(messages).as_ref() {
        let mut anchor = format!(
            "Current version anchor: {} [{}]",
            output.title, output.version_name
        );
        if !output.response.trim().is_empty() {
            anchor.push_str(&format!(
                " - {}",
                compact_text(&output.response, SUMMARY_ITEM_MAX_CHARS)
            ));
        }
        sections.push(anchor);
    }

    let recent_user_intents = messages
        .iter()
        .filter(|m| m.role == MessageRole::User)
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|m| format!("- {}", compact_text(&m.content, SUMMARY_ITEM_MAX_CHARS)))
        .collect::<Vec<_>>();
    if !recent_user_intents.is_empty() {
        sections.push(format!(
            "Recent user intents:\n{}",
            recent_user_intents.join("\n")
        ));
    }

    let recent_assistant_decisions = messages
        .iter()
        .filter(|m| m.role == MessageRole::Assistant)
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|m| {
            if let Some(output) = &m.output {
                let mut line = format!("{} [{}]", output.title, output.version_name);
                if !output.response.trim().is_empty() {
                    line.push_str(&format!(
                        " - {}",
                        compact_text(&output.response, SUMMARY_ITEM_MAX_CHARS)
                    ));
                }
                format!("- {}", line)
            } else {
                format!(
                    "- Q/A: {}",
                    compact_text(&m.content, SUMMARY_ITEM_MAX_CHARS)
                )
            }
        })
        .collect::<Vec<_>>();
    if !recent_assistant_decisions.is_empty() {
        sections.push(format!(
            "Recent assistant outcomes:\n{}",
            recent_assistant_decisions.join("\n")
        ));
    }

    compact_text(&sections.join("\n\n"), THREAD_SUMMARY_MAX_CHARS)
}

pub fn build_recent_dialogue(messages: &[Message]) -> String {
    messages
        .iter()
        .rev()
        .take(RECENT_DIALOGUE_MAX_MESSAGES)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|m| {
            let speaker = if m.role == MessageRole::User {
                "USER"
            } else {
                "ECKY EINACS"
            };
            format!(
                "{}: {}",
                speaker,
                compact_text(&m.content, RECENT_DIALOGUE_ITEM_MAX_CHARS)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn build_pinned_references_block(references: &[ThreadReference]) -> String {
    references
        .iter()
        .filter(|r| !r.content.trim().is_empty() || !r.summary.trim().is_empty())
        .rev()
        .take(PINNED_REFERENCES_MAX_ITEMS)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|r| {
            let body = if r.kind == "attachment_meta" {
                r.summary.clone()
            } else if !r.content.trim().is_empty() {
                compact_text(&r.content, PINNED_REFERENCE_CONTENT_MAX_CHARS)
            } else {
                r.summary.clone()
            };
            format!(
                "- {} [{}]\n{}\n",
                r.name,
                r.kind,
                compact_text(&body, PINNED_REFERENCE_CONTENT_MAX_CHARS)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub struct PromptContext {
    pub thread_id: String,
    pub thread_title: String,
    pub summary: String,
    pub recent_dialogue: String,
    pub pinned_references: String,
    pub available_assets: String,
    pub last_output: Option<DesignOutput>,
    pub design_digest: String,
    pub artifact_digest: String,
}

fn source_language_label(source_language: SourceLanguage) -> &'static str {
    match source_language {
        SourceLanguage::LegacyPython => "legacyPython",
        SourceLanguage::EckyIrV0 => "ecky",
        SourceLanguage::Build123d => "ecky",
    }
}

fn geometry_backend_label(geometry_backend: GeometryBackend) -> &'static str {
    match geometry_backend {
        GeometryBackend::Freecad => "freecad",
        GeometryBackend::Build123d => "eckyRust",
        GeometryBackend::EckyRust => "eckyRust",
    }
}

fn source_fence_label(source_language: SourceLanguage) -> &'static str {
    match source_language {
        SourceLanguage::EckyIrV0 => "scheme",
        SourceLanguage::LegacyPython => "python",
        SourceLanguage::Build123d => "scheme",
    }
}

fn resolved_context_from_design(design: &DesignOutput) -> ResolvedAuthoringContext {
    ResolvedAuthoringContext {
        engine_kind: design.engine_kind,
        source_language: design.source_language,
        geometry_backend: design.geometry_backend,
    }
}

fn format_authoring_context_lines(prefix: &str, context: ResolvedAuthoringContext) -> String {
    [
        format!("{prefix}EngineKind: {}", context.engine_kind.as_str()),
        format!(
            "{prefix}SourceLanguage: {}",
            source_language_label(context.source_language)
        ),
        format!(
            "{prefix}GeometryBackend: {}",
            geometry_backend_label(context.geometry_backend)
        ),
    ]
    .join("\n")
}

fn format_full_params_json(design: &DesignOutput) -> String {
    serde_json::to_string_pretty(&design.initial_params).unwrap_or_else(|_| "{}".to_string())
}

fn format_migration_policy(
    current: Option<ResolvedAuthoringContext>,
    target: ResolvedAuthoringContext,
) -> String {
    let mut lines = Vec::new();

    match current {
        Some(current_ctx) => {
            lines.push(
                "Preserve current authoring context unless the user explicitly asks to migrate."
                    .to_string(),
            );
            lines.push(
                "Normal iterations should continue in the thread's current source language/backend."
                    .to_string(),
            );
            if current_ctx != target {
                lines.push(format!(
                    "Current thread source is {} on {}. Selected target for this turn resolves to {} on {}.",
                    source_language_label(current_ctx.source_language),
                    geometry_backend_label(current_ctx.geometry_backend),
                    source_language_label(target.source_language),
                    geometry_backend_label(target.geometry_backend)
                ));
                lines.push(
                    "If config/defaults differ from current source and the request is ambiguous, ask one short clarification question instead of silently rewriting the whole model."
                        .to_string(),
                );
                lines.push(
                    "Do not migrate solely because defaults changed in Settings. Migrate only on explicit user intent or when the current task cannot be completed faithfully without migration."
                        .to_string(),
                );
            }
        }
        None => {
            lines.push(
                "No current thread source exists. Use TARGET AUTHORING CONTEXT for this turn."
                    .to_string(),
            );
        }
    }

    lines.join("\n")
}

pub fn assemble_context(
    db: &rusqlite::Connection,
    thread_id: Option<String>,
    working_design: Option<DesignOutput>,
    parent_macro_code: Option<String>,
) -> PromptContext {
    if let Some(tid) = thread_id {
        let messages = crate::db::get_thread_messages_for_context(db, &tid).unwrap_or_default();
        let latest_version = crate::db::get_thread_version_detail(db, &tid, None)
            .ok()
            .flatten()
            .map(|detail| detail.message);
        let latest_snapshot = latest_version
            .as_ref()
            .map(|message| latest_assistant_snapshot(std::slice::from_ref(message)))
            .unwrap_or_else(|| latest_assistant_snapshot(&messages));
        let mut summary_messages = messages.clone();
        if let Some(latest) = latest_version {
            if !summary_messages
                .iter()
                .any(|message| message.id == latest.id)
            {
                summary_messages.push(latest);
                summary_messages.sort_by_key(|message| message.timestamp);
            } else if let Some(existing) = summary_messages
                .iter_mut()
                .find(|message| message.id == latest.id)
            {
                *existing = latest;
            }
        }
        let summary = crate::db::get_thread_summary(db, &tid)
            .ok()
            .flatten()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| {
                build_thread_summary(
                    &crate::db::get_thread_title(db, &tid)
                        .ok()
                        .flatten()
                        .unwrap_or_default(),
                    &summary_messages,
                )
            });
        let dialogue = build_recent_dialogue(&messages);
        let title = crate::db::get_thread_title(db, &tid)
            .ok()
            .flatten()
            .unwrap_or_default();
        let refs = crate::db::get_thread_references(db, &tid).unwrap_or_default();

        let has_working_design = working_design.is_some();
        let last_output = working_design.or(latest_snapshot.output);
        let design_manifest = if has_working_design {
            None
        } else {
            latest_snapshot.model_manifest.as_ref()
        };
        let design_digest = build_design_digest(last_output.as_ref(), design_manifest);
        let artifact_digest = build_artifact_digest(latest_snapshot.artifact_bundle.as_ref());

        PromptContext {
            thread_id: tid,
            thread_title: title,
            summary,
            recent_dialogue: dialogue,
            pinned_references: build_pinned_references_block(&refs),
            available_assets: String::new(),
            last_output,
            design_digest,
            artifact_digest,
        }
    } else {
        let fallback_output = parent_macro_code.map(|code| {
            let macro_dialect = infer_macro_dialect_from_code(&code);
            let engine_kind = if macro_dialect == crate::contracts::MacroDialect::EckyIrV0 {
                crate::contracts::EngineKind::EckyIrV0
            } else {
                crate::contracts::EngineKind::Freecad
            };
            DesignOutput {
                title: "Untitled Design".to_string(),
                version_name: "V1".to_string(),
                response: String::new(),
                interaction_mode: InteractionMode::Design,
                macro_dialect,
                engine_kind,
                source_language: engine_kind.to_source_language(),
                geometry_backend: engine_kind.to_geometry_backend(),
                macro_code: code,
                ui_spec: UiSpec::default(),
                initial_params: Default::default(),
                post_processing: None,
            }
        });

        let last_output = working_design.or(fallback_output);

        PromptContext {
            thread_id: uuid::Uuid::new_v4().to_string(),
            thread_title: String::new(),
            summary: String::new(),
            recent_dialogue: String::new(),
            pinned_references: String::new(),
            available_assets: String::new(),
            design_digest: build_design_digest(last_output.as_ref(), None),
            artifact_digest: String::new(),
            last_output,
        }
    }
}

// ── 2.6: legacy optional sections behind the typed envelope ───────────────
//
// Compatibility bridge that routes the already-formatted legacy summary /
// dialogue / reference / asset strings through the typed `ContextEnvelope`
// (OpenSpec `agent-context-budgeting`, section 2.6). The envelope becomes the
// decision/audit layer for these four optional sections while the visible
// provider text stays byte-identical to the pre-envelope path: each legacy
// string is carried verbatim with no per-section budget, so the envelope
// records every populated section as `Included` with `returned == observed`.
//
// Re-projecting raw message/reference items through `project_sections` (with
// its own 4×200 / 2×1200 budgets) would change visible content and is deferred
// to section 3.4. System-prefix assembly, telemetry, and classification
// rewiring are likewise out of scope here.

/// Stable section ids for the four legacy optional sections routed behind the
/// envelope. They key the typed records so later stages (telemetry, full
/// routing) can refer to the same legacy slots.
pub mod legacy_section_id {
    pub const THREAD_SUMMARY: &str = "thread-summary";
    pub const RECENT_DIALOGUE: &str = "recent-dialogue";
    pub const PINNED_REFERENCES: &str = "pinned-references";
    pub const AVAILABLE_ASSETS: &str = "available-assets";
}

/// Build the four legacy optional sections from a [`PromptContext`], carrying
/// the legacy formatted strings verbatim. Empty sections are skipped (the
/// renderer substitutes `[none]`), matching the historical behaviour.
pub fn legacy_optional_sections(ctx: &PromptContext) -> Vec<ContextSection> {
    let mut sections = Vec::new();
    push_legacy_section(
        &mut sections,
        legacy_section_id::THREAD_SUMMARY,
        &ctx.summary,
        SectionPriority::Relevant,
        Sensitivity::Sensitive,
    );
    push_legacy_section(
        &mut sections,
        legacy_section_id::RECENT_DIALOGUE,
        &ctx.recent_dialogue,
        SectionPriority::Optional,
        Sensitivity::Sensitive,
    );
    push_legacy_section(
        &mut sections,
        legacy_section_id::PINNED_REFERENCES,
        &ctx.pinned_references,
        SectionPriority::Optional,
        Sensitivity::Sensitive,
    );
    push_legacy_section(
        &mut sections,
        legacy_section_id::AVAILABLE_ASSETS,
        &ctx.available_assets,
        SectionPriority::Optional,
        Sensitivity::Safe,
    );
    sections
}

fn push_legacy_section(
    out: &mut Vec<ContextSection>,
    id: &str,
    content: &str,
    priority: SectionPriority,
    sensitivity: Sensitivity,
) {
    if !content.trim().is_empty() {
        out.push(ContextSection::new(id, priority, sensitivity, content));
    }
}

/// Assemble the four legacy optional sections under the generation ceiling.
/// Because every legacy section is Relevant/Optional (truncatable) and carries
/// no per-section budget, exact content cannot overflow, so this never returns
/// a budget error for the legacy inputs.
pub fn assemble_legacy_optional_envelope(ctx: &PromptContext) -> ContextEnvelope {
    let sections = legacy_optional_sections(ctx);
    assemble_envelope(EnvelopeStage::Generation, sections)
        .expect("legacy optional sections are truncatable; exact content cannot overflow")
}

/// Visible value for one legacy optional slot: its verbatim content when the
/// envelope kept it, or `[none]` when it was absent (empty source) or omitted
/// by the budget. Consults the typed envelope decision so prompt formatting
/// sits behind the envelope.
pub fn legacy_optional_section_value(
    env: &ContextEnvelope,
    id: &str,
    fallback_content: &str,
) -> String {
    match env.record(id) {
        Some(record) if record.decision == InclusionDecision::Included => {
            fallback_content.to_string()
        }
        _ => "[none]".to_string(),
    }
}

/// Render the four legacy optional sections as the generation-prompt block
/// (THREAD SUMMARY / RECENT DIALOGUE / PINNED REFERENCES / AVAILABLE LOCAL
/// ASSETS), consulting the typed envelope for each slot's visible value.
/// Byte-identical to the historical `format_contextual_prompt` interpolation
/// for representative inputs.
pub fn render_legacy_generation_block(ctx: &PromptContext, env: &ContextEnvelope) -> String {
    let summary =
        legacy_optional_section_value(env, legacy_section_id::THREAD_SUMMARY, &ctx.summary);
    let dialogue = legacy_optional_section_value(
        env,
        legacy_section_id::RECENT_DIALOGUE,
        &ctx.recent_dialogue,
    );
    let references = legacy_optional_section_value(
        env,
        legacy_section_id::PINNED_REFERENCES,
        &ctx.pinned_references,
    );
    let assets = legacy_optional_section_value(
        env,
        legacy_section_id::AVAILABLE_ASSETS,
        &ctx.available_assets,
    );
    format!(
        "THREAD SUMMARY\n{summary}\n\nRECENT DIALOGUE\n{dialogue}\n\nPINNED REFERENCES (historical/supplemental; do not override ACTUAL CURRENT state unless the user asks)\n{references}\n\nAVAILABLE LOCAL ASSETS (AUTHORITATIVE; use absolute paths directly for image controls when relevant)\n{assets}"
    )
}

// ── 3.x: full generation context routed through the typed envelope ───────
//
// OpenSpec `agent-context-budgeting`, section 3. The generation user content is
// now assembled behind one typed `ContextEnvelope` covering the request, the
// exact current source/params (authoritative), the digests, and the four legacy
// optional sections. Static policy — the output contract, the shared language
// body, and the applicable CAD-framework rules — is NOT part of this envelope:
// it lives once in the stable system prefix (`design_system_prompt`). User
// content therefore carries current state and the current ask once, with no
// duplicate `EXECUTION RULES` block.
//
// Budget is enforced as a deterministic Unicode-character count under the 64K
// generation ceiling. Mandatory/authoritative (exact) state that cannot fit
// returns a raw `ContextBudgetError` (observed/allowed section sizes) so the
// command fails pre-dispatch instead of sending a lossy request. The envelope
// also carries total-size metadata covering the FULL rendered user content
// (sections plus the structural wrapper), so telemetry reflects the actual
// dispatched payload size.

/// Budgeted generation payload: the typed envelope (shape/decisions only) plus
/// the rendered, ceiling-validated user content ready for provider dispatch.
#[derive(Debug, Clone)]
pub struct GenerationPayload {
    pub envelope: ContextEnvelope,
    pub user_content: String,
}

/// Project the full variable generation context (request + exact current
/// source/params + digests + the four legacy optional sections) into budgeted
/// candidate sections. Static policy/framework are intentionally absent — they
/// belong to the stable system prefix.
pub fn generation_sections(
    ctx: &PromptContext,
    base_prompt: &str,
    target: ResolvedAuthoringContext,
) -> Vec<ContextSection> {
    let mut sections = Vec::new();

    // Mandatory: the actual user request.
    sections.push(ContextSection::new(
        section_id::REQUEST,
        SectionPriority::Mandatory,
        Sensitivity::Sensitive,
        base_prompt,
    ));

    // Mandatory: current/target authoring context + migration policy. Small but
    // authoritative; carried verbatim so it never competes with optional state.
    let current = ctx.last_output.as_ref().map(resolved_context_from_design);
    let mut authoring = String::new();
    authoring.push_str(&format_authoring_context_lines(
        "current",
        current.unwrap_or(target),
    ));
    authoring.push('\n');
    authoring.push_str(&format_authoring_context_lines("target", target));
    authoring.push('\n');
    authoring.push_str(&format_migration_policy(current, target));
    sections.push(ContextSection::new(
        section_id::AUTHORING_CONTEXT,
        SectionPriority::Mandatory,
        Sensitivity::Sensitive,
        authoring,
    ));

    // Authoritative: exact current source + params. Never silently truncated;
    // overflow is a pre-dispatch error.
    if let Some(previous) = &ctx.last_output {
        sections.push(ContextSection::new(
            section_id::CURRENT_SOURCE,
            SectionPriority::Authoritative,
            Sensitivity::Sensitive,
            &previous.macro_code,
        ));
        sections.push(ContextSection::new(
            section_id::CURRENT_PARAMS,
            SectionPriority::Authoritative,
            Sensitivity::Sensitive,
            format_full_params_json(previous),
        ));
    }

    // Relevant: digests (shape only for telemetry).
    push_legacy_section(
        &mut sections,
        section_id::DESIGN_DIGEST,
        &ctx.design_digest,
        SectionPriority::Relevant,
        Sensitivity::Safe,
    );
    push_legacy_section(
        &mut sections,
        section_id::ARTIFACT_DIGEST,
        &ctx.artifact_digest,
        SectionPriority::Relevant,
        Sensitivity::Safe,
    );

    // Optional: the four legacy optional sections, carried verbatim with their
    // historical priority/sensitivity and legacy stable ids (the renderer and
    // the 2.6 tests query these ids).
    sections.extend(legacy_optional_sections(ctx));

    sections
}

/// Assemble the generation user content behind the typed envelope. On mandatory
/// overflow returns a raw [`ContextBudgetError`]; on success returns the budget
/// envelope plus the rendered user content. The envelope's total-size metadata
/// is set to the rendered user-content length so it covers the full dispatched
/// payload (sections plus structural wrapper).
pub fn assemble_generation_payload(
    ctx: &PromptContext,
    base_prompt: &str,
    intent_mode: &str,
    target: ResolvedAuthoringContext,
) -> Result<GenerationPayload, ContextBudgetError> {
    assemble_generation_payload_with_catalog(ctx, base_prompt, intent_mode, target, "")
}

/// Keep retrieval optional and budgeted; exact current source always has priority.
pub fn assemble_generation_payload_with_catalog(
    ctx: &PromptContext,
    base_prompt: &str,
    intent_mode: &str,
    target: ResolvedAuthoringContext,
    catalog: &str,
) -> Result<GenerationPayload, ContextBudgetError> {
    let mut sections = generation_sections(ctx, base_prompt, target);
    if !catalog.is_empty() {
        sections.push(
            ContextSection::new(
                "catalog-candidates",
                SectionPriority::Optional,
                Sensitivity::Sensitive,
                catalog,
            )
            .with_budget(6200),
        );
    }
    let mut envelope = assemble_envelope(EnvelopeStage::Generation, sections)?;
    let mut user_content =
        render_generation_user_content(ctx, base_prompt, intent_mode, target, &envelope);
    if let Some(record) = envelope
        .records
        .iter_mut()
        .find(|record| record.id == "catalog-candidates")
    {
        if record.decision == InclusionDecision::Included
            && user_content
                .chars()
                .count()
                .saturating_add(catalog.chars().count())
                .saturating_add(2)
                <= envelope.ceiling_chars
        {
            user_content.push_str("\n\n");
            user_content.push_str(catalog);
        } else {
            record.decision = InclusionDecision::Omitted;
            record.returned_chars = 0;
            record.approx_returned_tokens = 0;
            record.reason = Some(crate::context_envelope::SectionReason::EnvelopeBudget);
        }
    }
    let rendered_chars = measure_chars(&user_content);
    envelope.total_returned_chars = rendered_chars;
    envelope.total_approx_returned_tokens = approx_tokens(rendered_chars);
    envelope.total_observed_chars = envelope.total_observed_chars.max(rendered_chars);
    Ok(GenerationPayload {
        envelope,
        user_content,
    })
}

/// Render the generation user content from a budgeted envelope. Static policy
/// (output contract / language / framework) is absent — it lives in the system
/// prefix — so this carries current state and the current ask once, with no
/// `EXECUTION RULES` duplicate. The four legacy optional sections consult the
/// envelope for their visible values.
fn render_generation_user_content(
    ctx: &PromptContext,
    base_prompt: &str,
    intent_mode: &str,
    target_authoring: ResolvedAuthoringContext,
    envelope: &ContextEnvelope,
) -> String {
    let full_prompt = format!(
        "USER REQUEST (ACTUAL)\n{}\n\nUSER_INTENT_MODE: {}",
        base_prompt, intent_mode
    );
    let available_assets_block = if ctx.available_assets.trim().is_empty() {
        "[none]".to_string()
    } else {
        ctx.available_assets.clone()
    };
    let current_authoring = ctx.last_output.as_ref().map(resolved_context_from_design);
    let current_authoring_block = current_authoring
        .map(|current| format_authoring_context_lines("current", current))
        .unwrap_or_else(|| "[none]".to_string());
    let target_authoring_block = format_authoring_context_lines("target", target_authoring);
    let migration_policy_block = format_migration_policy(current_authoring, target_authoring);

    if let Some(previous) = &ctx.last_output {
        let source_fence = source_fence_label(previous.source_language);
        // 3.4: the four legacy optional sections consult the budgeted generation
        // envelope (same stable ids as 2.6), so visible values sit behind the
        // typed budget decision.
        let summary_value = legacy_optional_section_value(
            envelope,
            legacy_section_id::THREAD_SUMMARY,
            &ctx.summary,
        );
        let dialogue_value = legacy_optional_section_value(
            envelope,
            legacy_section_id::RECENT_DIALOGUE,
            &ctx.recent_dialogue,
        );
        let references_value = legacy_optional_section_value(
            envelope,
            legacy_section_id::PINNED_REFERENCES,
            &ctx.pinned_references,
        );
        let assets_value = legacy_optional_section_value(
            envelope,
            legacy_section_id::AVAILABLE_ASSETS,
            &ctx.available_assets,
        );
        format!(
            "CURRENT DESIGN CONTEXT\nThread Title: {}\nCurrent Title: {}\nVersion: {}\n\nCURRENT AUTHORING CONTEXT (AUTHORITATIVE)\n{}\n\nTARGET AUTHORING CONTEXT (AUTHORITATIVE FOR THIS TURN)\n{}\n\nMIGRATION POLICY (AUTHORITATIVE)\n{}\n\nTHREAD SUMMARY\n{}\n\nRECENT DIALOGUE\n{}\n\nPINNED REFERENCES (historical/supplemental; do not override ACTUAL CURRENT state unless the user asks)\n{}\n\nAVAILABLE LOCAL ASSETS (AUTHORITATIVE; use absolute paths directly for image controls when relevant)\n{}\n\nACTUAL CURRENT DESIGN DIGEST (AUTHORITATIVE)\n{}\n\nACTUAL CURRENT ARTIFACT DIGEST (AUTHORITATIVE)\n{}\n\nACTUAL CURRENT PARAMS JSON (AUTHORITATIVE)\n```json\n{}\n```\n\nACTUAL CURRENT SOURCE (AUTHORITATIVE, NOT A SAMPLE):\nsourceLanguage: {}\nsourceFence: {}\n```{}\n{}\n```\n\n{}",
            ctx.thread_title,
            previous.title,
            previous.version_name,
            current_authoring_block,
            target_authoring_block,
            migration_policy_block,
            summary_value,
            dialogue_value,
            references_value,
            assets_value,
            if ctx.design_digest.trim().is_empty() { "[none]" } else { &ctx.design_digest },
            if ctx.artifact_digest.trim().is_empty() { "[none]" } else { &ctx.artifact_digest },
            format_full_params_json(previous),
            source_language_label(previous.source_language),
            source_fence,
            source_fence,
            previous.macro_code,
            full_prompt
        )
    } else {
        format!(
            "CURRENT AUTHORING CONTEXT (AUTHORITATIVE)\n{}\n\nTARGET AUTHORING CONTEXT (AUTHORITATIVE FOR THIS TURN)\n{}\n\nMIGRATION POLICY (AUTHORITATIVE)\n{}\n\nAVAILABLE LOCAL ASSETS (AUTHORITATIVE; use absolute paths directly for image controls when relevant)\n{}\n\n{}",
            current_authoring_block,
            target_authoring_block,
            migration_policy_block,
            available_assets_block,
            full_prompt
        )
    }
}

/// Assemble the generation user content behind the typed envelope. Returns the
/// rendered user content on success, or a raw [`ContextBudgetError`] when exact
/// mandatory/authoritative state cannot fit the generation ceiling (pre-dispatch
/// failure). Thin wrapper over [`assemble_generation_payload`].
pub fn format_contextual_prompt(
    ctx: &PromptContext,
    base_prompt: &str,
    intent_mode: &str,
    target_authoring: ResolvedAuthoringContext,
) -> Result<String, ContextBudgetError> {
    let payload = assemble_generation_payload(ctx, base_prompt, intent_mode, target_authoring)?;
    Ok(payload.user_content)
}

// ── 3.4: classification routed through its own compact typed envelope ───────
//
// OpenSpec `agent-context-budgeting`, decision 4. Intent classification uses its
// own 8,000-character projection: the design digest, the latest dialogue turn,
// and the frontend working snapshot. Full source/params/authoring policy never
// enter it, and pinned references enter only when attachment or reference
// intent is present. The actual user request travels separately to the
// classifier call, so it is not duplicated inside this context.

/// Assemble the classifier context behind its compact typed envelope. Returns
/// the rendered context string on success, or a [`ContextBudgetError`] when the
/// projected sections cannot fit the 8K classifier ceiling. `include_references`
/// is `true` only when attachment or reference intent is present.
pub fn assemble_classifier_context(
    ctx: &PromptContext,
    frontend_snapshot: Option<&str>,
    include_references: bool,
) -> Result<String, ContextBudgetError> {
    let inputs = ContextInputs {
        // The request travels separately to the classifier call; it is not part
        // of the thread context envelope.
        request: String::new(),
        digest: non_empty(ctx.design_digest.clone()),
        frontend_snapshot: frontend_snapshot
            .map(str::to_string)
            .filter(|value| !value.trim().is_empty()),
        // Latest dialogue turn only: split the formatted dialogue into lines so
        // the projection can take the last turn.
        dialogue: ctx
            .recent_dialogue
            .lines()
            .map(str::to_string)
            .filter(|line| !line.trim().is_empty())
            .collect(),
        references: if include_references {
            non_empty(ctx.pinned_references.clone())
                .into_iter()
                .collect()
        } else {
            Vec::new()
        },
        ..Default::default()
    };
    let (_stage, sections) =
        project_sections(ProjectionIntent::Classifier { include_references }, &inputs);
    let envelope = assemble_envelope(EnvelopeStage::Classifier, sections)?;
    Ok(render_classifier_context(&envelope, &inputs))
}

fn non_empty(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

/// Render the budgeted classifier context. Only included, non-empty sections are
/// emitted; the request section is skipped (it travels separately).
fn render_classifier_context(envelope: &ContextEnvelope, inputs: &ContextInputs) -> String {
    let mut blocks: Vec<String> = Vec::new();

    if let Some(digest) = included_content(
        envelope,
        section_id::DESIGN_DIGEST,
        inputs.digest.as_deref(),
    ) {
        blocks.push(format!(
            "ACTUAL LIVE DESIGN DIGEST (AUTHORITATIVE)\n{digest}"
        ));
    }
    if let Some(snapshot) = included_content(
        envelope,
        section_id::FRONTEND_SNAPSHOT,
        inputs.frontend_snapshot.as_deref(),
    ) {
        blocks.push(format!(
            "ACTUAL LIVE WORKING SNAPSHOT (FRONTEND)\n{snapshot}"
        ));
    }
    // Latest dialogue turn only.
    if let Some(record) = envelope.record(&section_id::dialogue(0)) {
        if record.decision == InclusionDecision::Included {
            if let Some(latest) = inputs.dialogue.last() {
                blocks.push(format!("RECENT DIALOGUE\n{latest}"));
            }
        }
    }
    if let Some(reference) = included_content(
        envelope,
        &section_id::reference(0),
        inputs.references.first().map(String::as_str),
    ) {
        blocks.push(format!("PINNED REFERENCES\n{reference}"));
    }

    blocks.join("\n\n")
}

fn included_content(envelope: &ContextEnvelope, id: &str, content: Option<&str>) -> Option<String> {
    let record = envelope.record(id)?;
    if record.decision != InclusionDecision::Included {
        return None;
    }
    let content = content?.trim();
    if content.is_empty() {
        return None;
    }
    Some(content.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{
        ArtifactBundle, DesignOutput, DocumentMetadata, EngineKind, EnrichmentStatus,
        ExportArtifact, GeometryBackend, ManifestEnrichmentState, Message, MessageStatus,
        ModelSourceKind, ParamValue, SourceLanguage,
    };

    fn mock_message(role: &str, content: &str, output: Option<DesignOutput>) -> Message {
        Message {
            id: "test-id".to_string(),
            role: role.parse().unwrap(),
            content: content.to_string(),
            status: MessageStatus::Success,
            output,
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            structural_verification: None,
            agent_origin: None,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
            timestamp: 1000,
        }
    }

    fn mock_design(title: &str) -> DesignOutput {
        DesignOutput {
            title: title.to_string(),
            version_name: "V1".to_string(),
            response: "Test response".to_string(),
            interaction_mode: InteractionMode::Design,
            macro_dialect: infer_macro_dialect_from_code("import FreeCAD"),
            engine_kind: crate::contracts::EngineKind::Freecad,
            source_language: crate::contracts::SourceLanguage::LegacyPython,
            geometry_backend: crate::contracts::GeometryBackend::Freecad,
            macro_code: "import FreeCAD".to_string(),
            ui_spec: UiSpec::default(),
            initial_params: Default::default(),
            post_processing: None,
        }
    }

    fn mock_artifact_bundle(
        model_id: &str,
        export_artifacts: Vec<ExportArtifact>,
    ) -> ArtifactBundle {
        ArtifactBundle {
            geometry_provenance: None,
            component_dependency_lock: None,
            component_dependency_lock_digest: None,
            component_import_origins: Vec::new(),
            component_placement_evidence: Vec::new(),
            schema_version: 1,
            model_id: model_id.to_string(),
            source_kind: ModelSourceKind::Generated,
            engine_kind: EngineKind::EckyIrV0,
            source_language: SourceLanguage::EckyIrV0,
            geometry_backend: GeometryBackend::EckyRust,
            content_hash: "hash".to_string(),
            artifact_version: 1,
            fcstd_path: String::new(),
            manifest_path: format!("/tmp/{model_id}/manifest.json"),
            macro_path: None,
            model_stl_path: format!("/tmp/{model_id}/model.stl"),
            viewer_assets: Vec::new(),
            edge_targets: Vec::new(),
            face_targets: Vec::new(),
            callout_anchors: Vec::new(),
            measurement_guides: Vec::new(),
            export_artifacts,
        }
    }

    fn mock_manifest(model_id: &str) -> ModelManifest {
        ModelManifest {
            geometry_provenance: None,
            component_import_origins: Vec::new(),
            component_placement_evidence: Vec::new(),
            schema_version: 1,
            model_id: model_id.to_string(),
            source_kind: ModelSourceKind::Generated,
            source_digest: None,
            core_digest: None,
            ast_schema_version: None,
            engine_kind: EngineKind::EckyIrV0,
            source_language: SourceLanguage::EckyIrV0,
            geometry_backend: GeometryBackend::EckyRust,
            document: DocumentMetadata {
                document_name: model_id.to_string(),
                document_label: model_id.to_string(),
                source_path: None,
                object_count: 0,
                warnings: Vec::new(),
            },
            parts: Vec::new(),
            parameter_groups: Vec::new(),
            control_primitives: Vec::new(),
            control_relations: Vec::new(),
            control_views: Vec::new(),
            preview_views: Vec::new(),
            advisories: Vec::new(),
            selection_targets: Vec::new(),
            measurement_annotations: Vec::new(),
            tagged_anchors: Default::default(),
            feature_graph: None,
            correspondence_graph: None,
            analysis_declarations: Vec::new(),
            warnings: Vec::new(),
            enrichment_state: ManifestEnrichmentState {
                status: EnrichmentStatus::None,
                proposals: Vec::new(),
            },
        }
    }

    fn step_export(path: &str) -> ExportArtifact {
        ExportArtifact {
            geometry_provenance: None,
            label: "STEP".to_string(),
            format: "step".to_string(),
            path: path.to_string(),
            role: "primary".to_string(),
        }
    }

    fn mock_design_with_authoring(
        title: &str,
        source_language: SourceLanguage,
        geometry_backend: GeometryBackend,
        macro_code: &str,
        initial_params: std::collections::BTreeMap<String, ParamValue>,
    ) -> DesignOutput {
        DesignOutput {
            title: title.to_string(),
            version_name: "V7".to_string(),
            response: "Test response".to_string(),
            interaction_mode: InteractionMode::Design,
            macro_dialect: infer_macro_dialect_from_code(macro_code),
            engine_kind: match source_language {
                SourceLanguage::EckyIrV0 => EngineKind::EckyIrV0,
                _ => EngineKind::Freecad,
            },
            source_language,
            geometry_backend,
            macro_code: macro_code.to_string(),
            ui_spec: UiSpec::default(),
            initial_params,
            post_processing: None,
        }
    }

    // --- compact_text ---

    #[test]
    fn compact_text_truncates_with_ellipsis() {
        let result = compact_text("hello world this is a long string", 10);
        assert_eq!(result.chars().count(), 10);
        assert!(result.ends_with('…'));
    }

    #[test]
    fn compact_text_noop_for_short_strings() {
        let result = compact_text("short", 100);
        assert_eq!(result, "short");
    }

    #[test]
    fn compact_text_collapses_whitespace() {
        let result = compact_text("hello    world\n\tfoo", 100);
        assert_eq!(result, "hello world foo");
    }

    #[test]
    fn compact_text_exact_boundary() {
        let result = compact_text("abcde", 5);
        assert_eq!(result, "abcde");
    }

    // --- build_thread_summary ---

    #[test]
    fn build_thread_summary_empty_messages() {
        let result = build_thread_summary("", &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn build_thread_summary_title_only() {
        let result = build_thread_summary("My Design", &[]);
        assert!(result.contains("Thread: My Design"));
    }

    #[test]
    fn build_thread_summary_with_user_and_assistant() {
        let messages = vec![
            mock_message("user", "Make a box", None),
            mock_message("assistant", "Here's a box", Some(mock_design("Box"))),
            mock_message("user", "Make it bigger", None),
        ];
        let result = build_thread_summary("Box Project", &messages);
        assert!(result.contains("Thread: Box Project"));
        assert!(result.contains("Make a box"));
        assert!(result.contains("Make it bigger"));
        assert!(result.contains("Box [V1]"));
    }

    // --- build_recent_dialogue ---

    #[test]
    fn build_recent_dialogue_empty() {
        let result = build_recent_dialogue(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn build_recent_dialogue_single_message() {
        let messages = vec![mock_message("user", "hello", None)];
        let result = build_recent_dialogue(&messages);
        assert_eq!(result, "USER: hello");
    }

    #[test]
    fn build_recent_dialogue_respects_max_limit() {
        let messages: Vec<Message> = (0..10)
            .map(|i| mock_message("user", &format!("msg {}", i), None))
            .collect();
        let result = build_recent_dialogue(&messages);
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), RECENT_DIALOGUE_MAX_MESSAGES);
        // Should contain the last 6 messages (indices 4-9)
        assert!(result.contains("msg 4"));
        assert!(result.contains("msg 9"));
        assert!(!result.contains("msg 3"));
    }

    // --- build_pinned_references_block ---

    #[test]
    fn build_pinned_references_block_empty() {
        let result = build_pinned_references_block(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn build_pinned_references_block_with_content() {
        let refs = vec![ThreadReference {
            id: "r1".to_string(),
            thread_id: "t1".to_string(),
            source_message_id: None,
            ordinal: 0,
            kind: "python_macro".to_string(),
            name: "test_macro".to_string(),
            content: "import FreeCAD".to_string(),
            summary: "A macro".to_string(),
            pinned: true,
            created_at: 1000,
        }];
        let result = build_pinned_references_block(&refs);
        assert!(result.contains("test_macro"));
        assert!(result.contains("[python_macro]"));
        assert!(result.contains("import FreeCAD"));
    }

    #[test]
    fn build_pinned_references_block_summary_only() {
        let refs = vec![ThreadReference {
            id: "r1".to_string(),
            thread_id: "t1".to_string(),
            source_message_id: None,
            ordinal: 0,
            kind: "attachment".to_string(),
            name: "file.stl".to_string(),
            content: "   ".to_string(),
            summary: "An STL file".to_string(),
            pinned: true,
            created_at: 1000,
        }];
        let result = build_pinned_references_block(&refs);
        assert!(result.contains("file.stl"));
        assert!(result.contains("An STL file"));
    }

    // --- latest_output ---

    #[test]
    fn latest_output_returns_last_assistant() {
        let messages = vec![
            mock_message("assistant", "first", Some(mock_design("First"))),
            mock_message("assistant", "second", Some(mock_design("Second"))),
        ];
        let result = latest_output(&messages).unwrap();
        assert_eq!(result.title, "Second");
    }

    #[test]
    fn latest_output_ignores_user_messages() {
        let design = mock_design("Only");
        let messages = vec![
            mock_message("assistant", "design", Some(design)),
            mock_message("user", "followup", None),
        ];
        let result = latest_output(&messages).unwrap();
        assert_eq!(result.title, "Only");
    }

    #[test]
    fn latest_output_handles_empty() {
        assert!(latest_output(&[]).is_none());
    }

    #[test]
    fn latest_output_none_when_no_outputs() {
        let messages = vec![mock_message("assistant", "just text", None)];
        assert!(latest_output(&messages).is_none());
    }

    #[test]
    fn latest_artifact_bundle_returns_latest_assistant_artifact() {
        let mut first = mock_message("assistant", "first", Some(mock_design("First")));
        first.artifact_bundle = Some(mock_artifact_bundle("model-first", Vec::new()));
        let mut second = mock_message("assistant", "second", Some(mock_design("Second")));
        second.artifact_bundle = Some(mock_artifact_bundle(
            "model-second",
            vec![step_export("/tmp/model-second/model.step")],
        ));

        let result = latest_artifact_bundle(&[first, second]).unwrap();

        assert_eq!(result.model_id, "model-second");
    }

    #[test]
    fn latest_assistant_snapshot_keeps_output_manifest_and_artifact_from_same_message() {
        let mut first = mock_message("assistant", "first", Some(mock_design("First")));
        first.model_manifest = Some(mock_manifest("model-first"));
        first.artifact_bundle = Some(mock_artifact_bundle("model-first", Vec::new()));
        let second = mock_message("assistant", "second", Some(mock_design("Second")));

        let snapshot = latest_assistant_snapshot(&[first, second]);

        assert_eq!(snapshot.output.unwrap().title, "Second");
        assert!(snapshot.model_manifest.is_none());
        assert!(snapshot.artifact_bundle.is_none());
    }

    #[test]
    fn build_artifact_digest_reports_step_truth_from_exports_only() {
        let no_step = build_artifact_digest(Some(&mock_artifact_bundle("mesh-only", Vec::new())));
        assert!(no_step.contains("hasStepExport: false"));
        assert!(no_step.contains("stepExportPath: [none]"));
        assert!(no_step.contains("edgeTargetCount: 0"));
        assert!(no_step.contains("faceTargetCount: 0"));

        let mut bundle =
            mock_artifact_bundle("cad-step", vec![step_export("/tmp/cad-step/model.step")]);
        bundle
            .edge_targets
            .push(crate::contracts::ViewerEdgeTarget {
                target_id: "body:edge:0:0-0-0_10-0-0".to_string(),
                durable_target_id: None,
                canonical_target_id: None,
                alias_ids: Vec::new(),
                part_id: "body".to_string(),
                viewer_node_id: "body".to_string(),
                label: "Body.Edge1".to_string(),
                editable: true,
                start: crate::contracts::ViewerEdgePoint {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                end: crate::contracts::ViewerEdgePoint {
                    x: 10.0,
                    y: 0.0,
                    z: 0.0,
                },
            });
        bundle
            .face_targets
            .push(crate::contracts::ViewerFaceTarget {
                target_id: "body:face:0:5-5-5:100".to_string(),
                durable_target_id: None,
                canonical_target_id: None,
                alias_ids: Vec::new(),
                part_id: "body".to_string(),
                viewer_node_id: "body".to_string(),
                label: "Body.Face1".to_string(),
                editable: true,
                center: crate::contracts::ViewerEdgePoint {
                    x: 5.0,
                    y: 5.0,
                    z: 5.0,
                },
                normal: Some([0.0, 0.0, 1.0]),
                area: Some(100.0),
            });
        let with_step = build_artifact_digest(Some(&bundle));
        assert!(with_step.contains("hasStepExport: true"));
        assert!(with_step.contains("stepExportPath: /tmp/cad-step/model.step"));
        assert!(with_step.contains("edgeTargetCount: 1"));
        assert!(with_step.contains("faceTargetCount: 1"));
    }

    #[test]
    fn format_contextual_prompt_marks_actual_state_as_authoritative() {
        let ctx = PromptContext {
            thread_id: "t1".to_string(),
            thread_title: "Thread A".to_string(),
            summary: "summary".to_string(),
            recent_dialogue: "USER: hi".to_string(),
            pinned_references: "ref".to_string(),
            available_assets: "- Ecky Family [PNG] path: /tmp/ecky-family.png".to_string(),
            last_output: Some(mock_design("Lens")),
            design_digest: "Current working snapshot\nLens [V1] (legacyPython)\n\nUI fields: 0"
                .to_string(),
            artifact_digest: build_artifact_digest(Some(&mock_artifact_bundle(
                "lens-runtime",
                vec![step_export("/tmp/lens-runtime/model.step")],
            ))),
        };

        let result = format_contextual_prompt(
            &ctx,
            "increase throat diameter",
            "DESIGN_EDIT",
            ResolvedAuthoringContext {
                engine_kind: EngineKind::Freecad,
                source_language: SourceLanguage::LegacyPython,
                geometry_backend: GeometryBackend::Freecad,
            },
        )
        .expect("normal-sized context assembles without overflow");

        assert!(result.contains("CURRENT AUTHORING CONTEXT (AUTHORITATIVE)"));
        assert!(result.contains("TARGET AUTHORING CONTEXT (AUTHORITATIVE FOR THIS TURN)"));
        assert!(result.contains("ACTUAL CURRENT SOURCE (AUTHORITATIVE, NOT A SAMPLE):"));
        assert!(result.contains("ACTUAL CURRENT PARAMS JSON (AUTHORITATIVE)"));
        assert!(result.contains("MIGRATION POLICY (AUTHORITATIVE)"));
        assert!(result.contains("ACTUAL CURRENT DESIGN DIGEST (AUTHORITATIVE)"));
        assert!(result.contains("ACTUAL CURRENT ARTIFACT DIGEST (AUTHORITATIVE)"));
        assert!(result.contains("hasStepExport: true"));
        assert!(result.contains("stepExportPath: /tmp/lens-runtime/model.step"));
        assert!(result.contains("AVAILABLE LOCAL ASSETS"));
        assert!(result.contains("USER REQUEST (ACTUAL)"));
        assert!(result.contains("USER_INTENT_MODE: DESIGN_EDIT"));
        // 3.2: static output policy lives once in the system prefix; the user
        // content no longer re-embeds it as an EXECUTION RULES block, and the
        // CAD-framework contract moved to the system prefix (see
        // `design_system_prompt`), so neither appears here.
        assert!(!result.contains("EXECUTION RULES"));
        assert!(!result.contains("ACTUAL CURRENT CAD FRAMEWORK"));
    }

    #[test]
    fn format_contextual_prompt_includes_migration_guidance_when_target_differs_from_current() {
        let ctx = PromptContext {
            thread_id: "t1".to_string(),
            thread_title: "Thread A".to_string(),
            summary: "summary".to_string(),
            recent_dialogue: "USER: continue".to_string(),
            pinned_references: String::new(),
            available_assets: String::new(),
            last_output: Some(mock_design_with_authoring(
                "Legacy Box",
                SourceLanguage::LegacyPython,
                GeometryBackend::Freecad,
                "import FreeCAD\nprint('legacy')",
                Default::default(),
            )),
            design_digest: "Current working snapshot\nLegacy Box [V7] (legacyPython)".to_string(),
            artifact_digest: String::new(),
        };

        let result = format_contextual_prompt(
            &ctx,
            "make wall thicker",
            "DESIGN_EDIT",
            ResolvedAuthoringContext {
                engine_kind: EngineKind::EckyIrV0,
                source_language: SourceLanguage::EckyIrV0,
                geometry_backend: GeometryBackend::Build123d,
            },
        )
        .expect("normal-sized context assembles without overflow");

        assert!(result.contains("currentSourceLanguage: legacyPython"));
        assert!(result.contains("targetSourceLanguage: ecky"));
        assert!(result.contains(
            "Preserve current authoring context unless the user explicitly asks to migrate."
        ));
        assert!(result.contains("If config/defaults differ from current source and the request is ambiguous, ask one short clarification question instead of silently rewriting the whole model."));
    }

    #[test]
    fn format_contextual_prompt_keeps_full_current_params_json_even_when_digest_truncates() {
        let initial_params = (1..=14)
            .map(|index| (format!("p{}", index), ParamValue::Number(index as f64)))
            .collect();
        let ctx = PromptContext {
            thread_id: "t1".to_string(),
            thread_title: "Thread A".to_string(),
            summary: String::new(),
            recent_dialogue: String::new(),
            pinned_references: String::new(),
            available_assets: String::new(),
            last_output: Some(mock_design_with_authoring(
                "Dense Params",
                SourceLanguage::EckyIrV0,
                GeometryBackend::EckyRust,
                "(model (part body (box p1 p2 p3)))\n",
                initial_params,
            )),
            design_digest: "Current working snapshot\nDense Params [V7] (ecky)\n\nCurrent params: 14\n- p1: number = 1\n- … 2 more params".to_string(),
            artifact_digest: String::new(),
        };

        let result = format_contextual_prompt(
            &ctx,
            "keep editing",
            "DESIGN_EDIT",
            ResolvedAuthoringContext {
                engine_kind: EngineKind::EckyIrV0,
                source_language: SourceLanguage::EckyIrV0,
                geometry_backend: GeometryBackend::EckyRust,
            },
        )
        .expect("normal-sized context assembles without overflow");

        assert!(result.contains("\"p1\": 1.0"));
        assert!(result.contains("\"p14\": 14.0"));
        assert!(result.contains("sourceFence: scheme"));
    }

    // --- 2.6: legacy optional sections routed behind the typed envelope ---

    use crate::context_envelope::{
        measure_chars, section_id, InclusionDecision, SectionPriority, Sensitivity,
        GENERATION_CEILING_CHARS,
    };

    fn legacy_optional_prompt_context() -> PromptContext {
        PromptContext {
            thread_id: "t1".to_string(),
            thread_title: "Thread A".to_string(),
            summary: "Thread: Box Project\n\nRecent user intents:\n- make a box".to_string(),
            recent_dialogue: "USER: make a box\nECKY EINACS: done".to_string(),
            pinned_references: "- macro [python_macro]\nimport FreeCAD\n".to_string(),
            available_assets: "- Ecky Family [PNG] path: /tmp/ecky.png".to_string(),
            last_output: Some(mock_design("Lens")),
            design_digest: "Current working snapshot\nLens [V1] (legacyPython)".to_string(),
            artifact_digest: String::new(),
        }
    }

    #[test]
    fn legacy_optional_sections_route_through_typed_envelope_without_changing_visible_content() {
        let ctx = legacy_optional_prompt_context();

        // 1. The four legacy sections are routed through the typed envelope with
        //    their historical priority/sensitivity and carried verbatim.
        let env = assemble_legacy_optional_envelope(&ctx);

        let summary = env
            .record(legacy_section_id::THREAD_SUMMARY)
            .expect("summary section must be projected");
        assert_eq!(summary.priority, SectionPriority::Relevant);
        assert_eq!(summary.sensitivity, Sensitivity::Sensitive);
        assert_eq!(summary.decision, InclusionDecision::Included);
        assert_eq!(summary.returned_chars, summary.observed_chars);
        assert_eq!(summary.observed_chars, measure_chars(&ctx.summary));

        let dialogue = env
            .record(legacy_section_id::RECENT_DIALOGUE)
            .expect("dialogue section must be projected");
        assert_eq!(dialogue.priority, SectionPriority::Optional);
        assert_eq!(dialogue.sensitivity, Sensitivity::Sensitive);
        assert_eq!(dialogue.decision, InclusionDecision::Included);
        assert_eq!(dialogue.observed_chars, measure_chars(&ctx.recent_dialogue));

        let references = env
            .record(legacy_section_id::PINNED_REFERENCES)
            .expect("references section must be projected");
        assert_eq!(references.priority, SectionPriority::Optional);
        assert_eq!(references.decision, InclusionDecision::Included);

        let assets = env
            .record(legacy_section_id::AVAILABLE_ASSETS)
            .expect("assets section must be projected");
        assert_eq!(assets.priority, SectionPriority::Optional);
        assert_eq!(assets.sensitivity, Sensitivity::Safe);
        assert_eq!(assets.decision, InclusionDecision::Included);

        // 2. Visible provider content is unchanged: the envelope-routed block is
        //    byte-identical to the historical format_contextual_prompt formula.
        let expected_block = format!(
            "THREAD SUMMARY\n{summary}\n\nRECENT DIALOGUE\n{dialogue}\n\nPINNED REFERENCES (historical/supplemental; do not override ACTUAL CURRENT state unless the user asks)\n{refs}\n\nAVAILABLE LOCAL ASSETS (AUTHORITATIVE; use absolute paths directly for image controls when relevant)\n{assets}",
            summary = ctx.summary,
            dialogue = ctx.recent_dialogue,
            refs = ctx.pinned_references,
            assets = ctx.available_assets,
        );
        let rendered_block = render_legacy_generation_block(&ctx, &env);
        assert_eq!(rendered_block, expected_block);

        // 3. That exact block appears verbatim inside the full generation prompt.
        let prompt = format_contextual_prompt(
            &ctx,
            "increase throat diameter",
            "DESIGN_EDIT",
            ResolvedAuthoringContext {
                engine_kind: EngineKind::Freecad,
                source_language: SourceLanguage::LegacyPython,
                geometry_backend: GeometryBackend::Freecad,
            },
        )
        .expect("normal-sized context assembles without overflow");
        assert!(
            prompt.contains(&rendered_block),
            "generation prompt must carry the envelope-routed legacy block verbatim"
        );
    }

    #[test]
    fn legacy_optional_envelope_substitutes_none_for_empty_sections() {
        let ctx = PromptContext {
            thread_id: "t1".to_string(),
            thread_title: "Thread A".to_string(),
            summary: String::new(),
            recent_dialogue: String::new(),
            pinned_references: String::new(),
            available_assets: String::new(),
            last_output: Some(mock_design("Lens")),
            design_digest: "digest".to_string(),
            artifact_digest: String::new(),
        };

        let env = assemble_legacy_optional_envelope(&ctx);

        // Empty source strings project no sections (the renderer substitutes
        // `[none]`), matching the historical format_contextual_prompt behaviour.
        assert!(env.record(legacy_section_id::THREAD_SUMMARY).is_none());
        assert!(env.record(legacy_section_id::RECENT_DIALOGUE).is_none());
        assert!(env.record(legacy_section_id::PINNED_REFERENCES).is_none());
        assert!(env.record(legacy_section_id::AVAILABLE_ASSETS).is_none());

        let block = render_legacy_generation_block(&ctx, &env);
        assert!(block.contains("THREAD SUMMARY\n[none]"));
        assert!(block.contains("RECENT DIALOGUE\n[none]"));
        assert!(
            block.contains("do not override ACTUAL CURRENT state unless the user asks)\n[none]")
        );
        assert!(block.contains(
            "AVAILABLE LOCAL ASSETS (AUTHORITATIVE; use absolute paths directly for image controls when relevant)\n[none]"
        ));
    }

    // ── 3.x: generation routed through the typed envelope ──────────────────

    fn generation_prompt_context(macro_code: &str) -> PromptContext {
        PromptContext {
            thread_id: "t1".to_string(),
            thread_title: "Thread A".to_string(),
            summary: "Thread summary".to_string(),
            recent_dialogue: "USER: hi\nECKY EINACS: hello".to_string(),
            pinned_references: String::new(),
            available_assets: String::new(),
            last_output: Some(mock_design_with_authoring(
                "Bracket",
                SourceLanguage::LegacyPython,
                GeometryBackend::Freecad,
                macro_code,
                Default::default(),
            )),
            design_digest: "Current working snapshot\nBracket [V1] (legacyPython)".to_string(),
            artifact_digest: String::new(),
        }
    }

    fn freecad_resolved() -> ResolvedAuthoringContext {
        ResolvedAuthoringContext {
            engine_kind: EngineKind::Freecad,
            source_language: SourceLanguage::LegacyPython,
            geometry_backend: GeometryBackend::Freecad,
        }
    }

    #[test]
    fn generation_sections_project_exact_source_and_params_as_authoritative() {
        let ctx = generation_prompt_context("import FreeCAD\nbox = Part.makeBox(1, 1, 1)\n");
        let sections = generation_sections(&ctx, "make it bigger", freecad_resolved());

        let request = sections
            .iter()
            .find(|s| s.id == section_id::REQUEST)
            .expect("request section projected");
        assert_eq!(request.priority, SectionPriority::Mandatory);

        let source = sections
            .iter()
            .find(|s| s.id == section_id::CURRENT_SOURCE)
            .expect("current-source section projected");
        assert_eq!(source.priority, SectionPriority::Authoritative);
        assert_eq!(source.sensitivity, Sensitivity::Sensitive);

        let params = sections
            .iter()
            .find(|s| s.id == section_id::CURRENT_PARAMS)
            .expect("current-params section projected");
        assert_eq!(params.priority, SectionPriority::Authoritative);
    }

    #[test]
    fn assemble_generation_payload_total_size_covers_rendered_user_content() {
        let ctx = generation_prompt_context("import FreeCAD\nbox = Part.makeBox(1, 1, 1)\n");
        let payload = assemble_generation_payload(
            &ctx,
            "increase the fillet radius",
            "DESIGN_EDIT",
            freecad_resolved(),
        )
        .expect("normal-sized context assembles without overflow");

        // Total-size metadata covers the FULL rendered user content (sections
        // plus the structural wrapper).
        assert_eq!(
            payload.envelope.total_returned_chars,
            measure_chars(&payload.user_content)
        );
        assert!(payload.envelope.total_returned_chars > 0);
    }

    #[test]
    fn assemble_generation_payload_fails_pre_dispatch_on_mandatory_overflow() {
        // Authoritative current source larger than the 64K generation ceiling.
        let overflow_source = "x".repeat(GENERATION_CEILING_CHARS + 6_000);
        let ctx = generation_prompt_context(&overflow_source);

        let result = assemble_generation_payload(
            &ctx,
            "edit the oversized model",
            "DESIGN_EDIT",
            freecad_resolved(),
        );
        let budget_err = result.expect_err("oversized authoritative source must overflow");

        assert_eq!(budget_err.allowed_chars, GENERATION_CEILING_CHARS);
        assert!(budget_err.observed_chars > GENERATION_CEILING_CHARS);
        // The raw error names the overflowing section and its observed size.
        let overflow_ids: Vec<&str> = budget_err
            .overflow_sections
            .iter()
            .map(|s| s.id.as_str())
            .collect();
        assert!(overflow_ids.contains(&section_id::CURRENT_SOURCE));
    }

    #[test]
    fn assemble_generation_payload_drops_no_duplicate_execution_rules() {
        let ctx = generation_prompt_context("import FreeCAD\nbox = Part.makeBox(1, 1, 1)\n");
        let payload =
            assemble_generation_payload(&ctx, "add a fillet", "DESIGN_EDIT", freecad_resolved())
                .expect("normal-sized context assembles without overflow");
        // The static output contract lives in the system prefix only; user
        // content no longer re-embeds it.
        assert!(!payload.user_content.contains("EXECUTION RULES"));
        assert!(payload
            .user_content
            .contains("USER_INTENT_MODE: DESIGN_EDIT"));
    }

    // ── 3.4: classifier context routed through its compact envelope ───────

    #[test]
    fn classifier_context_excludes_source_and_summary_and_includes_compact_state() {
        let ctx = generation_prompt_context("SECRET-CURRENT-SOURCE");
        let rendered = assemble_classifier_context(&ctx, Some("front snapshot"), false)
            .expect("classifier context assembles without overflow");

        // Design digest + latest dialogue + frontend snapshot are included.
        assert!(rendered.contains("ACTUAL LIVE DESIGN DIGEST (AUTHORITATIVE)"));
        assert!(rendered.contains("ACTUAL LIVE WORKING SNAPSHOT (FRONTEND)\nfront snapshot"));
        assert!(rendered.contains("RECENT DIALOGUE\nECKY EINACS: hello"));
        // Full source, params, thread summary, and authoring policy are excluded.
        assert!(!rendered.contains("SECRET-CURRENT-SOURCE"));
        assert!(!rendered.contains("THREAD SUMMARY"));
        // References excluded without reference intent.
        assert!(!rendered.contains("PINNED REFERENCES"));
    }

    #[test]
    fn classifier_context_includes_references_only_with_intent() {
        let mut ctx = generation_prompt_context("src");
        ctx.pinned_references = "- macro [python_macro]\nimport FreeCAD\n".to_string();

        let with_refs = assemble_classifier_context(&ctx, None, true)
            .expect("classifier context assembles without overflow");
        assert!(with_refs.contains("PINNED REFERENCES"));

        let without_refs = assemble_classifier_context(&ctx, None, false)
            .expect("classifier context assembles without overflow");
        assert!(!without_refs.contains("PINNED REFERENCES"));
    }
    #[test]
    fn given_retrieval_candidates_when_generation_is_assembled_then_untrusted_evidence_is_budgeted()
    {
        let ctx = generation_prompt_context("EXACT SOURCE");
        let candidate = "CATALOG CANDIDATES — UNTRUSTED DATA: pinned bracket sha256:exact";
        let payload = assemble_generation_payload_with_catalog(
            &ctx,
            "add mount",
            "DESIGN_EDIT",
            freecad_resolved(),
            candidate,
        )
        .unwrap();
        assert!(payload.user_content.contains(candidate));
        assert!(payload.user_content.contains("EXACT SOURCE"));
        assert!(payload
            .envelope
            .records
            .iter()
            .any(|section| section.id == "catalog-candidates"));
    }
}
