use crate::mcp::contracts::TargetAuthoringContext;
use crate::models::{DesignOutput, GeometryBackend, MacroDialect, SourceLanguage};

pub(crate) const ECKY_AUTHORING_CARD: &str = concat!(
    "Ecky authoring card\n",
    "- First read current target settings from `workspace_overview`, `target_macro_get`, or `target_detail_get`: `sourceLanguage`, `macroDialect`, `geometryBackend`.\n",
    "- Preserve model-version settings. For empty threads, use session config/defaults; threads do not own language/backend.\n",
    "- If `sourceLanguage=ecky`, write valid `.ecky` only; source starts with `(model ...)`.\n",
    "- For non-trivial source edits, use `macro_buffer_get` first, edit against line numbers/digest, then render with `macro_buffer_replace_and_preview`; use full `macro_preview_render` only for small complete rewrites or first versions.\n",
    "- Do not reuse parameter keys with different meanings. Keep macroCode, uiSpec, and initialParams aligned; remove stale params.\n",
    "- Valid basics: `(box 40 20 10 :align '(min center min))`, `(extrude (polygon ((0 0) (100 0) (100 20) (0 20))) 8)`, `(place (location (plane :origin '(80 0 6)) :rotate '(0 90 0)) (cylinder 4 18))`.\n",
    "- `let` is parallel; use `let*` when later bindings depend on earlier bindings.\n",
    "- Guide routing is dynamic. For `sourceLanguage=ecky`, read `ecky://guides/ecky-source` as the primary language guide. Read backend manifests only when checking a specific op/support question. Read prose backend guides only after a lowerer/render error or artifact/export claim.\n",
    "- Fillet/chamfer are topology-sensitive. If a selector matches no edges after one smaller-radius retry and one selector retry, stop retrying fillet/chamfer; rebuild the shape with rounded source geometry (`rounded-rect`, `rounded-polygon`, `offset-rounded`, `loft`, `taper`, `cone`, or explicit profiles).\n",
    "- Do not promise STEP unless current artifact truth says `hasStepExport=true` or exportArtifacts contains `format=step`; direct OCCT is internal, not a selectable user backend.\n",
    "- For an existing design target, call `thread_borrow`; for a brand-new design, call `thread_create`, then render the first version with `macro_preview_render`.\n",
    "- Render with `macro_preview_render`. If validation fails, surface exact raw error, fix source properly, and render again.\n",
    "- Persist successful previews with `commit_preview_version`; include returned `threadId`, `messageId`, and `modelId` in agent evidence.\n",
    "- Never write `history.sqlite` directly from scripts or agents. Version updates must flow through MCP tools only.\n",
    "- Verify geometry with `get_model_screenshot` after successful render.\n"
);

pub(crate) fn authoring_card_text() -> &'static str {
    ECKY_AUTHORING_CARD
}

pub(crate) fn target_authoring_context(design_output: &DesignOutput) -> TargetAuthoringContext {
    TargetAuthoringContext {
        source_language: design_output.source_language.as_str().to_string(),
        macro_dialect: macro_dialect_label(&design_output.macro_dialect).to_string(),
        geometry_backend: design_output.geometry_backend.as_str().to_string(),
        file_extension: file_extension(
            design_output.source_language,
            design_output.geometry_backend,
        )
        .to_string(),
        authoring_card: ECKY_AUTHORING_CARD.to_string(),
        guide_uris: guide_uris(
            design_output.source_language,
            design_output.geometry_backend,
        ),
    }
}

pub(crate) fn guide_uris(
    source_language: SourceLanguage,
    geometry_backend: GeometryBackend,
) -> Vec<String> {
    let mut uris = vec![
        "ecky://guides/authoring-card".to_string(),
        "ecky://guides/modeling-guidelines".to_string(),
    ];

    if source_language == SourceLanguage::EckyIrV0 {
        uris.push("ecky://guides/ecky-source".to_string());
        uris.push(match geometry_backend {
            GeometryBackend::Build123d => "ecky://guides/surface-manifest/build123d".to_string(),
            GeometryBackend::Freecad => "ecky://guides/surface-manifest/freecad".to_string(),
            GeometryBackend::EckyRust => "ecky://guides/surface-manifest/ecky-rust".to_string(),
        });
    } else {
        uris.push(match geometry_backend {
            GeometryBackend::Build123d => "ecky://guides/build123d".to_string(),
            GeometryBackend::Freecad => "ecky://guides/freecad".to_string(),
            GeometryBackend::EckyRust => "ecky://guides/ecky-rust".to_string(),
        });
    }

    uris
}

pub(crate) fn file_extension(
    source_language: SourceLanguage,
    geometry_backend: GeometryBackend,
) -> &'static str {
    match source_language {
        SourceLanguage::EckyIrV0 => ".ecky",
        SourceLanguage::Build123d => ".py",
        SourceLanguage::LegacyPython => match geometry_backend {
            GeometryBackend::Freecad => ".FCMacro",
            _ => ".py",
        },
    }
}

pub(crate) fn macro_dialect_label(dialect: &MacroDialect) -> &'static str {
    match dialect {
        MacroDialect::Legacy => "legacy",
        MacroDialect::CadFrameworkV1 => "cadFrameworkV1",
        MacroDialect::EckyIrV0 => "ecky",
        MacroDialect::Build123d => "build123d",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authoring_card_requires_artifact_truth_for_step_claims() {
        let card = authoring_card_text();

        assert!(card.contains("Do not promise STEP"));
        assert!(card.contains("hasStepExport=true"));
        assert!(card.contains("exportArtifacts contains `format=step`"));
        assert!(card.contains("direct OCCT is internal"));
    }

    #[test]
    fn authoring_card_stops_blind_fillet_retries() {
        let card = authoring_card_text();

        assert!(card.contains("Fillet/chamfer are topology-sensitive"));
        assert!(card.contains("selector matches no edges"));
        assert!(card.contains("stop retrying fillet/chamfer"));
        assert!(card.contains("rounded source geometry"));
    }
}
