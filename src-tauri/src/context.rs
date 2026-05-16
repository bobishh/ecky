use crate::llm_context::{build_authoring_digest, format_authoring_digest_text};
use crate::models::{
    infer_macro_dialect_from_code, ArtifactBundle, DesignOutput, EngineKind, GeometryBackend,
    InteractionMode, Message, MessageRole, ModelManifest, SourceLanguage, ThreadReference, UiSpec,
};

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
        format!("hasPreviewStl: {}", !bundle.preview_stl_path.trim().is_empty()),
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
        SourceLanguage::Build123d => "build123d",
    }
}

fn geometry_backend_label(geometry_backend: GeometryBackend) -> &'static str {
    match geometry_backend {
        GeometryBackend::Freecad => "freecad",
        GeometryBackend::Build123d => "build123d",
        GeometryBackend::EckyRust => "eckyRust",
    }
}

fn source_fence_label(source_language: SourceLanguage) -> &'static str {
    match source_language {
        SourceLanguage::EckyIrV0 => "scheme",
        SourceLanguage::LegacyPython | SourceLanguage::Build123d => "python",
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
        let last_o = latest_output(&messages);
        let last_manifest = latest_manifest(&messages);
        let last_artifact_bundle = latest_artifact_bundle(&messages);
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
                    &messages,
                )
            });
        let dialogue = build_recent_dialogue(&messages);
        let title = crate::db::get_thread_title(db, &tid)
            .ok()
            .flatten()
            .unwrap_or_default();
        let refs = crate::db::get_thread_references(db, &tid).unwrap_or_default();

        let last_output = working_design.or(last_o);
        let design_digest = build_design_digest(last_output.as_ref(), last_manifest.as_ref());
        let artifact_digest = build_artifact_digest(last_artifact_bundle.as_ref());

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
            let engine_kind = if macro_dialect == crate::models::MacroDialect::EckyIrV0 {
                crate::models::EngineKind::EckyIrV0
            } else {
                crate::models::EngineKind::Freecad
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

pub fn format_contextual_prompt(
    ctx: &PromptContext,
    base_prompt: &str,
    system_prompt: &str,
    intent_mode: &str,
    framework_contract: Option<&str>,
    target_authoring: ResolvedAuthoringContext,
) -> String {
    let framework_block = framework_contract
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| {
            format!(
                "ACTUAL CURRENT CAD FRAMEWORK (AUTHORITATIVE):\n```text\n{}\n```\n\n",
                value
            )
        })
        .unwrap_or_default();

    let full_prompt = format!(
        "USER REQUEST (ACTUAL)\n{}\n\nEXECUTION RULES (MANDATORY)\n{}\n\nUSER_INTENT_MODE: {}",
        base_prompt, system_prompt, intent_mode
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
        format!(
            "CURRENT DESIGN CONTEXT\nThread Title: {}\nCurrent Title: {}\nVersion: {}\n\nCURRENT AUTHORING CONTEXT (AUTHORITATIVE)\n{}\n\nTARGET AUTHORING CONTEXT (AUTHORITATIVE FOR THIS TURN)\n{}\n\nMIGRATION POLICY (AUTHORITATIVE)\n{}\n\nTHREAD SUMMARY\n{}\n\nRECENT DIALOGUE\n{}\n\nPINNED REFERENCES (historical/supplemental; do not override ACTUAL CURRENT state unless the user asks)\n{}\n\nAVAILABLE LOCAL ASSETS (AUTHORITATIVE; use absolute paths directly for image controls when relevant)\n{}\n\nACTUAL CURRENT DESIGN DIGEST (AUTHORITATIVE)\n{}\n\nACTUAL CURRENT ARTIFACT DIGEST (AUTHORITATIVE)\n{}\n\nACTUAL CURRENT PARAMS JSON (AUTHORITATIVE)\n```json\n{}\n```\n\nACTUAL CURRENT SOURCE (AUTHORITATIVE, NOT A SAMPLE):\nsourceLanguage: {}\nsourceFence: {}\n```{}\n{}\n```\n\n{}{}",
            ctx.thread_title,
            previous.title,
            previous.version_name,
            current_authoring_block,
            target_authoring_block,
            migration_policy_block,
            if ctx.summary.trim().is_empty() { "[none]" } else { &ctx.summary },
            if ctx.recent_dialogue.trim().is_empty() { "[none]" } else { &ctx.recent_dialogue },
            if ctx.pinned_references.trim().is_empty() { "[none]" } else { &ctx.pinned_references },
            available_assets_block,
            if ctx.design_digest.trim().is_empty() { "[none]" } else { &ctx.design_digest },
            if ctx.artifact_digest.trim().is_empty() { "[none]" } else { &ctx.artifact_digest },
            format_full_params_json(previous),
            source_language_label(previous.source_language),
            source_fence,
            source_fence,
            previous.macro_code,
            framework_block,
            full_prompt
        )
    } else {
        format!(
            "CURRENT AUTHORING CONTEXT (AUTHORITATIVE)\n{}\n\nTARGET AUTHORING CONTEXT (AUTHORITATIVE FOR THIS TURN)\n{}\n\nMIGRATION POLICY (AUTHORITATIVE)\n{}\n\nAVAILABLE LOCAL ASSETS (AUTHORITATIVE; use absolute paths directly for image controls when relevant)\n{}\n\n{}{}",
            current_authoring_block,
            target_authoring_block,
            migration_policy_block,
            available_assets_block,
            framework_block,
            full_prompt
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        ArtifactBundle, DesignOutput, EngineKind, ExportArtifact, GeometryBackend, Message,
        MessageStatus, ModelSourceKind, ParamValue, SourceLanguage,
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
            engine_kind: crate::models::EngineKind::Freecad,
            source_language: crate::models::SourceLanguage::LegacyPython,
            geometry_backend: crate::models::GeometryBackend::Freecad,
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
            preview_stl_path: format!("/tmp/{model_id}/preview.stl"),
            viewer_assets: Vec::new(),
            edge_targets: Vec::new(),
            face_targets: Vec::new(),
            callout_anchors: Vec::new(),
            measurement_guides: Vec::new(),
            export_artifacts,
        }
    }

    fn step_export(path: &str) -> ExportArtifact {
        ExportArtifact {
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
    fn build_artifact_digest_reports_step_truth_from_exports_only() {
        let no_step = build_artifact_digest(Some(&mock_artifact_bundle("mesh-only", Vec::new())));
        assert!(no_step.contains("hasStepExport: false"));
        assert!(no_step.contains("stepExportPath: [none]"));
        assert!(no_step.contains("edgeTargetCount: 0"));
        assert!(no_step.contains("faceTargetCount: 0"));

        let mut bundle =
            mock_artifact_bundle("cad-step", vec![step_export("/tmp/cad-step/model.step")]);
        bundle.edge_targets.push(crate::models::ViewerEdgeTarget {
            target_id: "body:edge:0:0-0-0_10-0-0".to_string(),
            durable_target_id: None,
            canonical_target_id: None,
            alias_ids: Vec::new(),
            part_id: "body".to_string(),
            viewer_node_id: "body".to_string(),
            label: "Body.Edge1".to_string(),
            editable: true,
            start: crate::models::ViewerEdgePoint {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            end: crate::models::ViewerEdgePoint {
                x: 10.0,
                y: 0.0,
                z: 0.0,
            },
        });
        bundle.face_targets.push(crate::models::ViewerFaceTarget {
            target_id: "body:face:0:5-5-5:100".to_string(),
            durable_target_id: None,
            canonical_target_id: None,
            alias_ids: Vec::new(),
            part_id: "body".to_string(),
            viewer_node_id: "body".to_string(),
            label: "Body.Face1".to_string(),
            editable: true,
            center: crate::models::ViewerEdgePoint {
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
            "rule block",
            "DESIGN_EDIT",
            Some("framework contract"),
            ResolvedAuthoringContext {
                engine_kind: EngineKind::Freecad,
                source_language: SourceLanguage::LegacyPython,
                geometry_backend: GeometryBackend::Freecad,
            },
        );

        assert!(result.contains("CURRENT AUTHORING CONTEXT (AUTHORITATIVE)"));
        assert!(result.contains("TARGET AUTHORING CONTEXT (AUTHORITATIVE FOR THIS TURN)"));
        assert!(result.contains("ACTUAL CURRENT SOURCE (AUTHORITATIVE, NOT A SAMPLE):"));
        assert!(result.contains("ACTUAL CURRENT PARAMS JSON (AUTHORITATIVE)"));
        assert!(result.contains("MIGRATION POLICY (AUTHORITATIVE)"));
        assert!(result.contains("ACTUAL CURRENT DESIGN DIGEST (AUTHORITATIVE)"));
        assert!(result.contains("ACTUAL CURRENT ARTIFACT DIGEST (AUTHORITATIVE)"));
        assert!(result.contains("hasStepExport: true"));
        assert!(result.contains("stepExportPath: /tmp/lens-runtime/model.step"));
        assert!(result.contains("ACTUAL CURRENT CAD FRAMEWORK (AUTHORITATIVE):"));
        assert!(result.contains("AVAILABLE LOCAL ASSETS"));
        assert!(result.contains("USER REQUEST (ACTUAL)"));
        assert!(result.contains("EXECUTION RULES (MANDATORY)"));
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
            "rule block",
            "DESIGN_EDIT",
            None,
            ResolvedAuthoringContext {
                engine_kind: EngineKind::EckyIrV0,
                source_language: SourceLanguage::EckyIrV0,
                geometry_backend: GeometryBackend::Build123d,
            },
        );

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
                SourceLanguage::Build123d,
                GeometryBackend::Build123d,
                "from build123d import *\nBox(1, 2, 3)\n",
                initial_params,
            )),
            design_digest: "Current working snapshot\nDense Params [V7] (build123d)\n\nCurrent params: 14\n- p1: number = 1\n- … 2 more params".to_string(),
            artifact_digest: String::new(),
        };

        let result = format_contextual_prompt(
            &ctx,
            "keep editing",
            "rule block",
            "DESIGN_EDIT",
            None,
            ResolvedAuthoringContext {
                engine_kind: EngineKind::Freecad,
                source_language: SourceLanguage::Build123d,
                geometry_backend: GeometryBackend::Build123d,
            },
        );

        assert!(result.contains("\"p1\": 1.0"));
        assert!(result.contains("\"p14\": 14.0"));
        assert!(result.contains("sourceFence: python"));
    }
}
