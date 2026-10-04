use base64::{engine::general_purpose, Engine as _};
use std::collections::BTreeMap;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, State};
use uuid::Uuid;

use crate::context::*;
use crate::contracts::{
    validate_design_output, AppError, AppErrorCode, AppResult, ArtifactBundle, Attachment,
    AttachmentKind, DesignOutput, FinalizeStatus, GenerateDesignOptions, GenerateOutput,
    IntentDecision, InteractionMode, MacroDialect, Message, MessageRole, MessageStatus,
    ModelManifest, StructuralVerificationResult, UiSpec, UsageSummary,
};
use crate::models::AppState;
use crate::services::design::{auto_heal_legacy_params, is_param_schema_mismatch};
use crate::services::session::{build_saved_version_snapshot, write_last_snapshot};
use crate::{
    db, fallback_intent, freecad, llm, persist_thread_summary, persist_user_prompt_references,
    TECHNICAL_SYSTEM_PROMPT,
};

/// Per-language documentation appended to the API-mode system prompt.
///
/// FreeCAD Python is publicly documented, so a short recall note plus
/// the app-specific runtime contract is enough. Ecky is proprietary and unknown to
/// models, so a compact authoring guide is embedded.
pub fn language_guide_text(
    source_language: crate::contracts::SourceLanguage,
    geometry_backend: crate::contracts::GeometryBackend,
) -> String {
    match source_language {
        crate::contracts::SourceLanguage::EckyIrV0 => format!(
            "Ecky is a proprietary in-app CAD language. It is NOT publicly documented; \
             this guide and the example below are the complete authoritative reference. \
             Do not invent forms, ops, or keywords beyond what is listed here.\n\n{}\n\
             Prefer the smallest model that satisfies the request, then add named structure.\n",
            crate::agent_prompt::agent_language_reference(geometry_backend)
        ),
        crate::contracts::SourceLanguage::Build123d => {
            "build123d source/runtime was removed. Migrate this target to canonical `.ecky` source and Ecky Native.".to_string()
        }
        crate::contracts::SourceLanguage::LegacyPython => freecad_python_guide_text(),
    }
}

/// Static system prefix for API-mode design generation. Carries, once and in a
/// stable order so provider exact-prefix caching can reuse it:
///   1. the output/behaviour contract (`TECHNICAL_SYSTEM_PROMPT`);
///   2. the shared single-source language body (`language_guide_text`);
///   3. the applicable static CAD-framework rules, when provided.
///
/// Variable thread state and the current ask live in user content
/// (`format_contextual_prompt`), after this prefix. The CAD-framework contract is
/// passed by the caller only for applicable FreeCAD/CAD-SDK contexts (task 3.3);
/// it is `None` (omitted) for Ecky requests where it is irrelevant.
pub fn design_system_prompt(
    source_language: crate::contracts::SourceLanguage,
    geometry_backend: crate::contracts::GeometryBackend,
    framework_contract: Option<&str>,
) -> String {
    let framework_block = framework_contract
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            format!(
                "\n\nACTUAL CURRENT CAD FRAMEWORK (AUTHORITATIVE):\n```text\n{}\n```",
                value
            )
        })
        .unwrap_or_default();
    format!(
        "{}\n\n{}\n\nTARGET LANGUAGE GUIDE (AUTHORITATIVE FOR `macro_code`)\n{}{}",
        TECHNICAL_SYSTEM_PROMPT,
        crate::exploration_prompt::STATIC_GUIDANCE,
        language_guide_text(source_language, geometry_backend),
        framework_block
    )
}

pub fn freecad_python_guide_text() -> String {
    concat!(
        "Return a FreeCAD Python macro in `macro_code`.\n",
        "Current fileExtension: `.FCMacro`.\n",
        "Current sourceLanguage: `python` (FreeCAD).\n",
        "FreeCAD's Python API (`FreeCAD`/`App`, `Part`) is publicly documented; use standard, well-known API only.\n\n",
        "Runtime contract:\n",
        "- The macro runs headless inside FreeCAD with an active empty document already created.\n",
        "- Dynamic parameters are injected as a `params` dict (alias `parameters`); each key is also injected as a top-level variable. Always read with safe defaults: `radius = params.get('radius', 10.0)`.\n",
        "- Build solids with `Part` (e.g. `Part.makeBox`, `Part.makeCylinder`, booleans via `.cut`/`.fuse`/`.common`).\n",
        "- Expose finished solids either by adding `Part::Feature` objects to the active document, or by assigning `_ecky_parts = [('label', shape)]` (list of tuples: label, shape).\n",
        "- All dimensions are millimeters. Keep solids manifold and printable.\n"
    )
    .to_string()
}

pub fn ecky_ir_v0_guide_text(backend: crate::contracts::GeometryBackend) -> String {
    crate::agent_prompt::agent_language_reference(backend)
}

pub fn build123d_guide_text() -> String {
    "build123d was removed. Migrate to `.ecky` with Ecky Native.".to_string()
}

pub fn build123d_python_guide_text() -> String {
    build123d_guide_text()
}

#[allow(dead_code)]
fn ecky_backend_guide_text(
    backend: crate::contracts::GeometryBackend,
    backend_label: &str,
    implicit_backend: bool,
) -> String {
    let surface = crate::ecky_language_surface::supported_surface_manifest(backend);
    let target_line = if implicit_backend {
        "Target geometryBackend comes from current request, model version, or config default; never from thread metadata.\n".to_string()
    } else {
        format!("Target geometryBackend: `{backend_label}`.\n")
    };
    let backend_note = if matches!(backend, crate::contracts::GeometryBackend::EckyRust) {
        "- `mesh`/`eckyRust` renders through EckyRust CAD VM. Do not promise STEP unless `ArtifactBundle.exportArtifacts` proves one exists.\n"
    } else {
        "- This is still `.ecky` source. Backend only selects lowerer/runtime behavior; never emit Python source for `.ecky` requests. Wall-pattern is mesh/eckyRust only; it rejects on this backend.\n"
    };
    let wall_patterns = if surface.wall_pattern_modes.is_empty() {
        String::new()
    } else {
        format!(
            "- Mesh-only `wall-pattern` is available here. Named `:mode` values: {}.\n",
            crate::ecky_language_surface::join_backticked(surface.wall_pattern_modes)
        )
    };
    let supported_ops = crate::ecky_language_surface::join_backticked(&surface.cad_ops);
    let model_clauses = crate::ecky_language_surface::join_backticked(surface.model_clauses);
    let core_constants = crate::ecky_language_surface::join_backticked(surface.core_constants);
    let expression_forms = crate::ecky_language_surface::join_backticked(surface.expression_forms);
    let component_placement_forms =
        crate::ecky_language_surface::join_backticked(surface.component_placement_forms);
    let numeric_helpers = crate::ecky_language_surface::join_backticked(surface.numeric_helpers);
    let point_helpers = crate::ecky_language_surface::join_backticked(surface.point_list_helpers);

    format!(
        "Return canonical Ecky source in `macro_code`.\n\
    Current fileExtension: `.ecky`.\n\
    Current sourceLanguage: `ecky`.\n\
    {target_line}\
    Start every renderable answer with `(model ...)`.\n\n\
    API MODE ONE-PROMPT WORKFLOW\n\
    - Treat this target-language guide as the complete source of truth for `.ecky`; API mode cannot call MCP tools, fetch web docs, or inspect external resources.\n\
    - First derive named params and fit-critical bindings from the request; then write geometry; then write top-level verification clauses for measurable promises.\n\
    - Author verification clauses, but do not claim you ran them. The app runs compile, render, structural verification, and authored verify after `macro_code` returns.\n\
    - If repairing a failed generation, keep existing verify intent and strengthen geometry/params until checks pass; never delete the check to hide failure.\n\
    - Return JSON only per the outer contract, with complete Ecky source in `macro_code`.\n\n\
    AUTHORING RULES\n\
    - Output finished renderable geometry unless user explicitly asks for a placeholder. {typed_hole_policy}\n\
    - Direct model clauses: {model_clauses}. Put them directly under `model` or inside a supported model wrapper.\n\
- Supported expression forms: {expression_forms}. Use `let*` when later bindings depend on earlier ones.
\
- Component placement forms: {component_placement_forms}. Author reusable bodies in local coordinates; mate them with `place-component` and named ports instead of deriving Euler angles.
\
    - Never use `(define ...)` inside `(model ...)`. Steel evaluates it eagerly before params have values, producing a misleading TypeMismatch. Use `let*` inside `(part ...)` for computed values from params: `(part body (let* ((half (/ width 2))) (box half 10 10)))`. Top-level `(define (fn args) ...)` helper functions outside `(model ...)` are allowed for reusable pure functions.\n\
    - Use `map`, `range`, `repeat-union`, and `repeat-compound` inside geometry, not to generate top-level clauses.\n\
    - Static tuple destructuring is supported only for `zip` and `enumerate` static sources: `(map (lambda ((x y)) ...) (zip xs ys))`. Zip destructuring of a dynamic source rejects with a clear error.\n\
    - Supported CAD ops for this backend: {supported_ops}.\n\
    - Core constants: {core_constants}. Numeric helpers: {numeric_helpers}. Point/list helpers: {point_helpers}. Bounded literal counts/steps only. Seeded helpers are deterministic for a given seed.\n\
    - Keywords are not callable nodes: write `(box 10 10 2 :align '(center center min))`, never `(align ...)`.\n\
    - Name fit-critical bindings before use: `wall`, `clearance`, `bore-r`, `top-z`. No anonymous offsets for fit-critical geometry.\n\
    - For generated Ecky models, write direct `(verify ...)` clauses under the top-level `(model ...)` from the user's measurable requirements before trusting geometry; a red first render is expected repair input.\n\
    - Verify with typed/static errors and structural verification first, screenshots last.\n\
    {backend_note}{wall_patterns}\n\
    PARAMS\n\
    - `(number key default :label \"...\" :min n :max n :step n)`\n\
    - `(select key \"default\" :label \"...\" :options ((\"Label\" \"value\") ...))`\n\
    - `(toggle key #t :label \"...\")`\n\
    - `(image key \"\" :label \"...\")`\n\n\
    VERIFY CLAUSES\n\
    - Purpose: make source carry machine-checkable intent. The model writes `(verify ...)`; app verification evaluates it later.\n\
    - Put model verification directly under `model`, before or after `part` clauses. Never nest it inside geometry, params, `build`, or `let`. A `define-component` may carry its own verification clauses before its single geometry body; they travel with each instance.\n\
    - Clause grammar: `(verify (tag stable-name optional.selector ...) [(intent \"why\")] [(severity error|warning)] [(when bool-expr)] (metric alias (namespace key optional.args ...)) (expect alias (operator literal)))`.\n\
    - `tag`, `metric`, and `expect` are required exactly once. Optional `intent`, `severity`, and `when` may each appear once. Section order is free; emitted source is canonical. Empty `(verify)` is invalid.\n\
    - Severity defaults to `error`. A failed `warning` expectation stays visible but does not block; invalid syntax/evaluation still blocks. `when` accepts boolean literals/params and nested `not`, `and`, `or`; false emits `skipped` without evaluating metric/expect.\n\
    - `tag` carries authored labels or selectors for diagnostics; use stable names like `mesh_clean`, `lid_gap`, `preview_exists`.\n\
    - `metric` first item is a local alias; second item is a metric expression. `expect` alias must match the metric alias exactly.\n\
    - Metric namespaces: `manifest`, `stl`, `clearance`, `selector`, `relation`.\n\
    - Manifest metrics: `(manifest has-step)`, `(manifest has-model-stl)`, `(manifest edge-target-count)`, `(manifest face-target-count)`, `(manifest export-format-count)`, `(manifest part-count)`.\n\
    - STL metrics: `(stl triangle-count)`, `(stl connected-component-count)`, `(stl non-manifold-edge-count)`, `(stl overhang-face-count)`, `(stl bed-contact-area-ratio [part-id])`, `(stl bed-contact-x-span-ratio [part-id])`, `(stl bed-contact-y-span-ratio [part-id])`. Bed-contact metrics compare downward planar faces touching the lowest Z plane against all downward planar faces; use them when print-bed grounding matters.\n\
    - Clearance metric: `(clearance min-distance selector-a selector-b)`. Selectors may be part names such as `body` and `lid`, or stable target ids when known.\n\
    - Selector metrics: `(selector axis selector)`, `(selector extent-x selector)`, `(selector extent-y selector)`, `(selector extent-z selector)`, `(selector center-x selector)`, `(selector center-y selector)`, `(selector center-z selector)`. Axis returns text: `x`, `y`, or `z`; extents and centers are millimeters.\n\
    - Relation metrics: `(relation axis-angle selector-a selector-b)`, `(relation center-delta-x selector-a selector-b)`, `(relation center-delta-y selector-a selector-b)`, `(relation center-delta-z selector-a selector-b)`. Axis angle is unsigned degrees; center deltas are signed millimeters: selector-a center minus selector-b center.\n\
    - Operators: `=`, `!=`, `>`, `>=`, `<`, `<=`. Literals may be boolean, number, or text; do not use params or computed expressions in `expect` literals.\n\
    - Good default checks for generated `.ecky`: model STL exists, part count is positive, STL triangle count is positive, non-manifold edge count is zero.\n\
    - Use clearance verification when the request names a fit/gap/clearance. Use numeric literals matching the promised clearance.\n\
    - Use selector/relation verification when the request names orientation, fit axis, length, width, thickness, center offset, or perpendicular/parallel relation.\n\
    - Do not remove or weaken existing `(verify ...)` clauses during repair; change geometry or params until they pass.\n\
    ```ecky\n\
    (model\n\
    (verify\n\
    (tag preview_exists)\n\
    (metric check (manifest has-model-stl))\n\
    (expect check (= true)))\n\
    (verify\n\
    (tag mesh_clean)\n\
    (metric bad_edges (stl non-manifold-edge-count))\n\
    (expect bad_edges (= 0)))\n\
    (verify\n\
    (tag lid_clearance body lid)\n\
    (metric gap (clearance min-distance body lid))\n\
    (expect gap (>= 0.3)))\n\
    (verify\n\
    (tag part_count)\n\
    (metric parts (manifest part-count))\n\
    (expect parts (>= 2)))\n\
    (verify\n\
    (tag joint_axis joint_tongue)\n\
    (metric axis (selector axis joint_tongue))\n\
    (expect axis (= \"y\")))\n\
    (verify\n\
    (tag joint_width joint_tongue)\n\
    (metric width (selector extent-x joint_tongue))\n\
    (expect width (>= 11.8)))\n\
    (verify\n\
    (tag tube_joint_perpendicular tube_axis joint_tongue)\n\
    (metric angle (relation axis-angle tube_axis joint_tongue))\n\
    (expect angle (>= 85)))\n\
    (part body (box 30 20 10))\n\
    (part lid (translate 0 0 10.4 (box 30 20 2))))\n\
    ```\n\n\
    PROGRESSIVE ECKY EXAMPLES\n\n\
    1. First solid:\n\
    ```ecky\n\
    (model\n\
    (part marker\n\
    (sphere 10)))\n\
    ```\n\n\
    2. Sketch then extrude:\n\
    ```ecky\n\
    (model\n\
    (part plate\n\
    (extrude (rounded-rect 70 42 5) 4)))\n\
    ```\n\n\
    3. Sketch with a hole:\n\
    ```ecky\n\
    (model\n\
    (part washer\n\
    (extrude\n\
      (profile :outer (rounded-rect 70 42 5)\n\
               :holes (circle 9 64))\n\
      4)))\n\
    ```\n\n\
    4. Parameters, named stages, and cuts:\n\
    ```ecky\n\
    (model\n\
    (params\n\
    (number plate-w 80 :label \"Plate width\" :min 40 :max 120)\n\
    (number plate-h 48 :label \"Plate height\" :min 20 :max 80)\n\
    (number hole-r 4 :label \"Hole radius\" :min 2 :max 8))\n\
    (part mount\n\
    (build\n\
      (shape blank (extrude (rounded-rect plate-w plate-h 4) 5))\n\
      (shape left-hole (translate -24 0 -0.5 (cylinder hole-r 6)))\n\
      (shape right-hole (translate 24 0 -0.5 (cylinder hole-r 6)))\n\
      (result (difference blank left-hole right-hole)))))\n\
    ```\n\n\
    5. Repetition instead of copy-paste:\n\
    ```ecky\n\
    (model\n\
    (part ribbed-plate\n\
    (build\n\
      (shape base (box 90 40 4))\n\
      (shape ribs\n\
        (repeat-union i 5\n\
          (translate (- (* i 18) 36) 0 5 (box 4 34 6))))\n\
      (result (union base ribs)))))\n\
    ```\n\n\
    6. Final-pattern model: plate + bore + clipped thread + repeated features:\n\
    ```ecky\n\
    (model\n\
    (params\n\
    (number lens-bore-d 59.6 :label \"Lens bore D\" :min 50 :max 68)\n\
    (number clearance 0.25 :label \"Thread clearance\" :min 0.1 :max 0.6))\n\
    (part adapter\n\
    (build\n\
      (shape bore-r (/ lens-bore-d 2))\n\
      (shape carrier\n\
        (extrude\n\
          (profile :outer (rounded-rect 96 62 6)\n\
                   :holes (rounded-rect 72 38 3))\n\
          4))\n\
      (shape socket\n\
        (difference\n\
          (translate 0 0 4 (cylinder (+ bore-r 5) 24))\n\
          (translate 0 0 3.5 (cylinder (+ bore-r clearance) 25))))\n\
      (shape thread-a\n\
        (clip-box\n\
          (translate 0 0 7\n\
            (helical-ridge :radius bore-r :pitch 4 :height 18\n\
                           :base-width 1.2 :crest-width 0.55 :depth 0.9\n\
                           :female #t :clearance clearance))\n\
          :x (-36 36) :y (-36 36) :z (7 25)))\n\
      (shape thread-b (rotate 0 0 180 thread-a))\n\
      (shape ribs\n\
        (repeat-union i 5\n\
          (translate (- (* i 18) 36) 31 6 (box 5 8 8))))\n\
      (result (union carrier socket thread-a thread-b ribs)))))\n\
    ```\n\n\
    READING ORDER FOR GENERATED CODE\n\
    Start primitive or sketch. Add params. Add named `build` stages. Add booleans. Add repetition/placement. Add verification clauses for measurable model invariants before final JSON.\n",
        typed_hole_policy = surface.typed_hole_policy,
    )
}

pub fn freecad_guide_text() -> String {
    crate::agent_prompt::agent_language_reference(crate::contracts::GeometryBackend::Freecad)
}

pub fn ecky_source_guide_text() -> String {
    crate::agent_prompt::agent_language_reference(crate::contracts::GeometryBackend::EckyRust)
}

fn selected_engine_core(state: &AppState) -> AppResult<crate::contracts::Engine> {
    let config = state.config.lock().unwrap();
    let engine = config
        .engines
        .iter()
        .find(|candidate| candidate.id == config.selected_engine_id)
        .cloned()
        .ok_or_else(|| AppError::validation("No active engine selected."))?;

    if engine.provider != "ollama" && engine.api_key.trim().is_empty() {
        return Err(AppError::validation(format!(
            "Selected engine '{}' has no API key configured.",
            engine.name
        )));
    }

    Ok(engine)
}

fn selected_engine(state: &State<'_, AppState>) -> AppResult<crate::contracts::Engine> {
    selected_engine_core(state.inner())
}

fn default_engine_kind(app_state: &AppState) -> crate::contracts::EngineKind {
    app_state.config.lock().unwrap().default_engine_kind
}

fn default_source_language(app_state: &AppState) -> crate::contracts::SourceLanguage {
    app_state.config.lock().unwrap().default_source_language
}

fn default_geometry_backend(app_state: &AppState) -> crate::contracts::GeometryBackend {
    app_state.config.lock().unwrap().default_geometry_backend
}

async fn resolve_generation_engine_kind(
    app_state: &AppState,
    _thread_id: Option<&str>,
    explicit: Option<crate::contracts::EngineKind>,
    working_design: Option<&DesignOutput>,
    last_output: Option<&DesignOutput>,
) -> AppResult<crate::contracts::EngineKind> {
    if let Some(engine_kind) = explicit {
        return Ok(engine_kind);
    }

    if let Some(design) = working_design {
        return Ok(design.engine_kind);
    }

    if let Some(design) = last_output {
        return Ok(design.engine_kind);
    }

    Ok(default_engine_kind(app_state))
}

async fn resolve_generation_source_language(
    app_state: &AppState,
    _thread_id: Option<&str>,
    explicit: Option<crate::contracts::SourceLanguage>,
    working_design: Option<&DesignOutput>,
    last_output: Option<&DesignOutput>,
) -> AppResult<crate::contracts::SourceLanguage> {
    if let Some(source_language) = explicit {
        return Ok(source_language);
    }

    if let Some(design) = working_design {
        return Ok(design.source_language);
    }

    if let Some(design) = last_output {
        return Ok(design.source_language);
    }

    Ok(default_source_language(app_state))
}

async fn resolve_generation_geometry_backend(
    app_state: &AppState,
    _thread_id: Option<&str>,
    explicit: Option<crate::contracts::GeometryBackend>,
    working_design: Option<&DesignOutput>,
    last_output: Option<&DesignOutput>,
) -> AppResult<crate::contracts::GeometryBackend> {
    if let Some(geometry_backend) = explicit {
        return Ok(geometry_backend);
    }

    if let Some(design) = working_design {
        return Ok(design.geometry_backend);
    }

    if let Some(design) = last_output {
        return Ok(design.geometry_backend);
    }

    Ok(default_geometry_backend(app_state))
}

fn prepare_images(image_data: Option<String>, attachments: Option<Vec<Attachment>>) -> Vec<String> {
    let mut images = Vec::new();
    if let Some(main_image) = image_data {
        images.push(main_image);
    }
    if let Some(attachments) = attachments {
        for attachment in attachments {
            if attachment.kind == AttachmentKind::Image {
                if let Some(data_url) = attachment_image_data_url(&attachment) {
                    images.push(data_url);
                }
            }
        }
    }
    images
}

fn attachment_image_data_url(attachment: &Attachment) -> Option<String> {
    if let Some(data_url) = attachment
        .data_url
        .as_deref()
        .map(str::trim)
        .filter(|value| value.starts_with("data:image/"))
    {
        return Some(data_url.to_string());
    }
    let bytes = fs::read(&attachment.path).ok()?;
    let b64 = general_purpose::STANDARD.encode(bytes);
    let ext = attachment
        .path
        .split('.')
        .next_back()
        .unwrap_or("png")
        .to_lowercase();
    let mime = match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        _ => "image/png",
    };
    Some(format!("data:{};base64,{}", mime, b64))
}

fn collect_attachment_images(attachments: Option<&Vec<Attachment>>) -> Vec<String> {
    attachments
        .into_iter()
        .flat_map(|items| items.iter())
        .filter(|attachment| attachment.kind == AttachmentKind::Image)
        .filter_map(attachment_image_data_url)
        .collect()
}

#[cfg(test)]
mod attachment_image_tests {
    use super::*;

    #[test]
    fn attachment_image_data_url_prefers_inline_payload_over_path_reads() {
        let attachment = Attachment {
            path: "/definitely/missing.png".to_string(),
            name: "missing.png".to_string(),
            explanation: String::new(),
            data_url: Some("data:image/png;base64,Zm9v".to_string()),
            kind: AttachmentKind::Image,
        };

        assert_eq!(
            attachment_image_data_url(&attachment).as_deref(),
            Some("data:image/png;base64,Zm9v")
        );
    }

    #[test]
    fn attachment_image_data_url_preserves_svg_mime_type() {
        let path = std::env::temp_dir().join(format!("ecky-svg-{}.svg", Uuid::new_v4()));
        fs::write(&path, b"<svg/>").expect("svg fixture");
        let attachment = Attachment {
            path: path.to_string_lossy().to_string(),
            name: "overlay.svg".to_string(),
            explanation: String::new(),
            data_url: None,
            kind: AttachmentKind::Image,
        };

        let data_url = attachment_image_data_url(&attachment).expect("svg data url");

        assert!(data_url.starts_with("data:image/svg+xml;base64,"));
        let _ = fs::remove_file(path);
    }
}

fn build_visual_input_notes(
    image_data: Option<&String>,
    attachments: Option<&Vec<Attachment>>,
) -> Option<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut image_index = 1usize;

    if image_data.is_some() {
        lines.push(format!(
            "Image {} is the current 3D viewport screenshot.",
            image_index
        ));
        lines.push(
            "If it contains colored strokes, arrows, circles, or hand-drawn marks, treat them as explicit user annotations highlighting the intended area or requested change."
                .to_string(),
        );
        image_index += 1;
    }

    if let Some(attachments) = attachments {
        for attachment in attachments {
            if attachment.kind != AttachmentKind::Image {
                continue;
            }
            let explanation = attachment.explanation.trim();
            if explanation.is_empty() {
                lines.push(format!(
                    "Image {} is attachment '{}' from the user.",
                    image_index, attachment.name
                ));
            } else {
                lines.push(format!(
                    "Image {} is attachment '{}' from the user. User note: {}",
                    image_index, attachment.name, explanation
                ));
            }
            image_index += 1;
        }
    }

    if lines.is_empty() {
        None
    } else {
        Some(format!("VISUAL INPUTS\n{}", lines.join("\n")))
    }
}

fn build_available_assets_block(state: &AppState, app: &dyn crate::models::PathResolver) -> String {
    let mut by_path = BTreeMap::new();
    {
        let config = state.config.lock().unwrap();
        for asset in &config.assets {
            let format = asset.format.trim().to_uppercase();
            if !matches!(format.as_str(), "PNG" | "JPG" | "JPEG" | "WEBP") {
                continue;
            }
            by_path
                .entry(asset.path.clone())
                .or_insert_with(|| asset.clone());
        }
    }

    if let Ok(scanned) = crate::commands::assets::collect_image_assets(app) {
        for asset in scanned {
            by_path.entry(asset.path.clone()).or_insert(asset);
        }
    }

    let mut assets = by_path.into_values().collect::<Vec<_>>();
    assets.sort_by_key(|asset| asset.name.to_lowercase());
    assets
        .into_iter()
        .take(8)
        .map(|asset| format!("- {} [{}] path: {}", asset.name, asset.format, asset.path))
        .collect::<Vec<_>>()
        .join("\n")
}

fn load_framework_contract(app: &dyn crate::models::PathResolver) -> Option<String> {
    let path = freecad::resolve_resource_path(
        app,
        "model-runtime/cad_framework_contract.md",
        &[
            "../model-runtime/cad_framework_contract.md",
            "model-runtime/cad_framework_contract.md",
        ],
    )
    .ok()?;
    fs::read_to_string(path).ok()
}

/// Whether the CAD-SDK framework contract applies to this generation
/// (OpenSpec `agent-context-budgeting`, task 3.3).
///
/// The FreeCAD CAD-SDK contract is relevant only for FreeCAD-based authoring
/// (legacy Python, or a current macro already using the CAD framework). Ecky and
/// build123d have their own language surfaces and never use the FreeCAD CAD-SDK,
/// so the framework block is omitted for them to avoid irrelevant policy in the
/// stable system prefix. A new thread with no current output defaults to the
/// FreeCAD CAD-SDK path (the historical default).
fn should_use_framework_for_generation(ctx: &PromptContext) -> bool {
    !matches!(
        ctx.last_output
            .as_ref()
            .map(|output| output.source_language),
        Some(crate::contracts::SourceLanguage::EckyIrV0)
            | Some(crate::contracts::SourceLanguage::Build123d)
    )
}

fn prepend_follow_up_context(prompt: String, follow_up_question: Option<&str>) -> String {
    let Some(question) = follow_up_question
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return prompt;
    };

    format!(
        "FOLLOW-UP CONTEXT (AUTHORITATIVE)\nThe assistant previously asked this unresolved clarification question:\n\"{}\"\n\nThe current user request is the answer to that question. Continue the pending design task using the user's answer. Do not repeat the same clarification question unless the new answer is still genuinely insufficient.\n\n{}",
        question, prompt
    )
}

fn exploration_phase(
    phase: crate::contracts::exploration_cycle::CyclePhase,
) -> crate::exploration_prompt::CyclePhase {
    use crate::contracts::exploration_cycle::CyclePhase as Stored;
    use crate::exploration_prompt::CyclePhase as Prompt;
    match phase {
        Stored::Planning | Stored::AwaitingInput | Stored::Idle => Prompt::Plan,
        Stored::Building => Prompt::Build,
        Stored::Verifying => Prompt::Verify,
        Stored::Deciding => Prompt::Decide,
    }
}

fn cycle_required_output(phase: crate::contracts::exploration_cycle::CyclePhase) -> &'static str {
    use crate::contracts::exploration_cycle::CyclePhase;
    match phase {
        CyclePhase::Planning => "exactly one typed PLAN action; no executable plan tail",
        CyclePhase::Building => "one bounded source change derived from the exact current version",
        CyclePhase::Verifying => "no authored source; wait for deterministic verification evidence",
        CyclePhase::Deciding => "one typed COMPLETE, REPLAN, ASK, STOP, or COMPARE decision",
        CyclePhase::AwaitingInput => "one concise clarification answer from the user",
        CyclePhase::Idle => "no next output; cycle is not running",
    }
}

fn active_cycle_envelope(
    conn: &rusqlite::Connection,
    thread_id: &str,
) -> AppResult<Option<String>> {
    let Some((packet, _)) =
        crate::exploration_store::load_latest_active_cycle_for_thread(conn, thread_id)
            .map_err(|error| AppError::persistence(error.to_string()))?
    else {
        return Ok(None);
    };
    let (input_digest, status): (Option<String>, String) = conn
        .query_row(
            "SELECT version_input_digest, status FROM messages
             WHERE id = ?1 AND thread_id = ?2 AND output IS NOT NULL AND deleted_at IS NULL",
            rusqlite::params![packet.state.current_version_id, thread_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| AppError::persistence(error.to_string()))?;
    let deterministic_failed = packet
        .last_verification
        .as_ref()
        .is_some_and(|verification| {
            verification.deterministic
                == crate::contracts::exploration_cycle::VerificationVerdict::Red
        });
    let evidence_items = packet
        .last_verification
        .as_ref()
        .map(|verification| {
            vec![crate::exploration_prompt::VerificationEvidence {
                kind: crate::exploration_prompt::EvidenceKind::RedVerification,
                severity: if deterministic_failed {
                    crate::exploration_prompt::EvidenceSeverity::Error
                } else {
                    crate::exploration_prompt::EvidenceSeverity::Info
                },
                code: verification.evidence_ref.clone(),
                raw: format!(
                    "versionId={} inputDigest={} evidenceRef={} deterministic={:?}",
                    verification.version_id,
                    verification.input_digest,
                    verification.evidence_ref,
                    verification.deterministic
                ),
            }]
        })
        .unwrap_or_default();
    let context = crate::exploration_prompt::ExplorationCycleContext {
        cycle_id: packet.state.cycle_id,
        goal: packet.definition.objective,
        acceptance_criteria: packet.definition.acceptance_criteria,
        hard_constraints: packet.definition.hard_constraints,
        soft_preferences: packet.definition.soft_preferences,
        current_version_id: packet.state.current_version_id,
        current_version_input_digest: input_digest.unwrap_or_else(|| "unavailable".to_string()),
        current_version_status: status,
        last_verification_evidence: crate::exploration_prompt::EvidenceState {
            items: evidence_items,
            consecutive_red_verifications: u32::from(deterministic_failed),
            deterministic_failed,
        },
        last_answer: packet.state.last_answer,
        remaining_budget: crate::exploration_prompt::CycleBudget {
            remaining_actions: packet.state.budget.saturating_sub(packet.state.budget_used),
            remaining_seconds: None,
            remaining_tokens: None,
        },
        current_phase: exploration_phase(packet.state.phase),
        required_next_output: cycle_required_output(packet.state.phase).to_string(),
        prompt_version: packet.prompt_version,
    };
    crate::exploration_prompt::render_cycle_envelope(&context)
        .map(Some)
        .map_err(|error| validation_prompt_error(error.to_string()))
}

fn validation_prompt_error(message: String) -> AppError {
    AppError::with_details(
        AppErrorCode::Validation,
        "Exploration prompt assembly failed.",
        message,
    )
}

fn enforce_final_generation_budget(prompt: String) -> AppResult<String> {
    let observed = prompt.chars().count();
    let allowed = crate::context_envelope::GENERATION_CEILING_CHARS;
    if observed > allowed {
        return Err(AppError::with_details(
            AppErrorCode::Validation,
            "Context budget overflow after cycle/follow-up/visual context; refusing to dispatch a lossy request.",
            format!("observedChars={observed}; allowedChars={allowed}"),
        ));
    }
    Ok(prompt)
}

fn record_cycle_model_event(
    conn: &rusqlite::Connection,
    thread_id: &str,
    engine: &crate::contracts::Engine,
    usage: Option<&UsageSummary>,
    latency_ms: u64,
    raw_error: Option<String>,
) -> AppResult<()> {
    let Some((mut packet, mut build_started)) =
        crate::exploration_store::load_latest_active_cycle_for_thread(conn, thread_id)
            .map_err(|error| AppError::persistence(error.to_string()))?
    else {
        return Ok(());
    };
    packet.event_count += 1;
    let route = crate::contracts::exploration_cycle::CycleRouteMetadata {
        prompt_version: packet.prompt_version.clone(),
        provider: engine.provider.clone(),
        model: engine.model.clone(),
        reasoning_effort: None,
        latency_ms: Some(latency_ms),
        input_tokens: usage.map(|value| value.input_tokens),
        output_tokens: usage.map(|value| value.output_tokens),
        estimated_cost_usd: usage.and_then(|value| value.estimated_cost_usd),
    };
    packet.last_route = Some(route.clone());
    let failed = raw_error.is_some();
    if failed {
        let mut reducer = crate::exploration_cycle::CycleReducer::restore(
            packet.state.clone(),
            packet.last_verification.clone(),
            build_started,
        );
        reducer
            .apply(crate::exploration_cycle::Transition::ProviderFailed)
            .map_err(|error| {
                AppError::validation(format!(
                    "Invalid provider failure transition while recording model route: {error:?}"
                ))
            })?;
        packet.state = reducer.state().clone();
        build_started = reducer.build_started();
    }
    let event = crate::contracts::exploration_cycle::CycleEvent {
        event_id: Uuid::new_v4().to_string(),
        cycle_id: packet.state.cycle_id.clone(),
        sequence: packet.event_count,
        event_type: if failed {
            crate::contracts::exploration_cycle::CycleEventType::ProviderFailed
        } else {
            crate::contracts::exploration_cycle::CycleEventType::ModelCallRecorded
        },
        phase: packet.state.phase,
        source_version_id: Some(packet.state.current_version_id.clone()),
        result_version_id: None,
        evidence_ref: None,
        raw_error,
        render_snapshot_id: None,
        artifact_digest: None,
        route: Some(route),
        plan: None,
        question: None,
        blocked_decision: None,
        answer: None,
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };
    crate::exploration_store::save_transition(conn, &packet, build_started, &event)
        .map_err(|error| AppError::persistence(error.to_string()))
}

#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
pub async fn generate_design(
    prompt: String,
    thread_id: Option<String>,
    parent_macro_code: Option<String>,
    working_design: Option<DesignOutput>,
    _is_retry: bool,
    image_data: Option<String>,
    attachments: Option<Vec<Attachment>>,
    options: Option<GenerateDesignOptions>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<GenerateOutput> {
    generate_design_core(
        GenerateDesignCoreInput {
            prompt,
            thread_id,
            parent_macro_code,
            working_design,
            is_retry: _is_retry,
            image_data,
            attachments,
            options,
        },
        state.inner(),
        &app,
    )
    .await
}

/// Inputs for one provider authoring call.
///
/// The Tauri command is only a boundary adapter. Keeping this input separate
/// lets a Rust-owned exploration actor invoke the exact same generation path
/// without reconstructing frontend state or calling a command through Tauri.
#[derive(Debug, Clone)]
pub(crate) struct GenerateDesignCoreInput {
    pub prompt: String,
    pub thread_id: Option<String>,
    pub parent_macro_code: Option<String>,
    pub working_design: Option<DesignOutput>,
    pub is_retry: bool,
    pub image_data: Option<String>,
    pub attachments: Option<Vec<Attachment>>,
    pub options: Option<GenerateDesignOptions>,
}

pub(crate) async fn generate_design_core(
    input: GenerateDesignCoreInput,
    state: &AppState,
    app: &dyn crate::models::PathResolver,
) -> AppResult<GenerateOutput> {
    let GenerateDesignCoreInput {
        prompt,
        thread_id,
        parent_macro_code,
        working_design,
        is_retry: _is_retry,
        image_data,
        attachments,
        options,
    } = input;
    {
        let no_enabled_engine = {
            let config = state.config.lock().unwrap();
            !config.engines.iter().any(|e| e.enabled)
        };

        if no_enabled_engine {
            let sessions = state.mcp_session_registry.with_sessions().lock().await;
            if sessions.is_empty() {
                return Err(AppError::validation(
                    "No active engine or MCP agent found. Switch to API Key mode in Settings → Agents, or connect an agent.",
                ));
            }
            return Err(AppError::validation(
                "A Rust-owned exploration run requires an enabled API engine. An active external MCP agent cannot be invoked recursively from its own tool call.",
            ));
        }
    }
    let engine = selected_engine_core(state)?;
    let options = options.unwrap_or_default();
    let (mut ctx, cycle_envelope) = {
        let db = state.db.lock().await;
        let ctx = crate::context::assemble_context(
            &db,
            thread_id.clone(),
            working_design.clone(),
            parent_macro_code.clone(),
        );
        let cycle_envelope = active_cycle_envelope(&db, &ctx.thread_id)?;
        (ctx, cycle_envelope)
    };
    let engine_kind = resolve_generation_engine_kind(
        state,
        thread_id.as_deref(),
        options.engine_kind,
        working_design.as_ref(),
        ctx.last_output.as_ref(),
    )
    .await?;
    let source_language = resolve_generation_source_language(
        state,
        thread_id.as_deref(),
        options.source_language,
        working_design.as_ref(),
        ctx.last_output.as_ref(),
    )
    .await?;
    let geometry_backend = resolve_generation_geometry_backend(
        state,
        thread_id.as_deref(),
        options.geometry_backend,
        working_design.as_ref(),
        ctx.last_output.as_ref(),
    )
    .await?;
    let question_mode = options.question_mode.unwrap_or(false);
    let follow_up_question = options.follow_up_question;
    ctx.available_assets = build_available_assets_block(state, app);
    let intent_mode = if question_mode {
        "QUESTION_ONLY"
    } else {
        "DESIGN_EDIT"
    };
    let framework_enabled = should_use_framework_for_generation(&ctx)
        && source_language == crate::contracts::SourceLanguage::LegacyPython;
    let framework_contract = if framework_enabled {
        load_framework_contract(app)
    } else {
        None
    };
    let target_authoring = crate::context::ResolvedAuthoringContext {
        engine_kind,
        source_language,
        geometry_backend,
    };
    // 3.2/3.4: route the generation user content through the typed envelope.
    // The static output contract / language / framework live once in the system
    // prefix; user content carries current state + the current ask once (no
    // duplicate EXECUTION RULES). Mandatory/authoritative state that cannot fit
    // the generation ceiling fails here, pre-dispatch, with a raw context-budget
    // error naming observed/allowed section sizes — never a lossy request.
    // 4.2: assemble the payload directly (rather than the `format_contextual_
    // prompt` thin wrapper) so the budgeted envelope is retained for safe,
    // content-free telemetry recorded after dispatch through the existing
    // profiler/session-activity path. The rendered user content is byte-
    // identical; this is the only generation-assembly hook telemetry needs.
    let generation_payload = crate::context::assemble_generation_payload(
        &ctx,
        &prompt,
        intent_mode,
        target_authoring,
    )
    .map_err(|budget_err| {
        AppError::with_details(
            AppErrorCode::Validation,
            "Context budget overflow: mandatory authoritative state cannot fit the generation ceiling; refusing to dispatch a lossy request.",
            serde_json::to_string(&budget_err).unwrap_or_else(|_| budget_err.to_string()),
        )
    })?;
    let contextual_prompt = if let Some(ref envelope) = cycle_envelope {
        format!("{}\n\n{}", generation_payload.user_content, envelope)
    } else {
        generation_payload.user_content
    };
    let contextual_prompt =
        prepend_follow_up_context(contextual_prompt, follow_up_question.as_deref());
    let contextual_prompt =
        if let Some(notes) = build_visual_input_notes(image_data.as_ref(), attachments.as_ref()) {
            format!("{}\n\n{}", contextual_prompt, notes)
        } else {
            contextual_prompt
        };
    let contextual_prompt = enforce_final_generation_budget(contextual_prompt)?;
    let images = prepare_images(image_data, attachments);

    let system_prompt = design_system_prompt(
        source_language,
        geometry_backend,
        framework_contract.as_deref(),
    );
    let model_started = std::time::Instant::now();
    let output =
        match llm::generate_design(&engine, &system_prompt, &contextual_prompt, images).await {
            Ok(output) => output,
            Err(raw_body) => {
                let db = state.db.lock().await;
                record_cycle_model_event(
                    &db,
                    &ctx.thread_id,
                    &engine,
                    None,
                    model_started.elapsed().as_millis() as u64,
                    Some(raw_body.clone()),
                )?;
                return Err(AppError::with_details(
                    AppErrorCode::Provider,
                    "LLM response could not be parsed into a design output.",
                    raw_body,
                ));
            }
        };
    let next_action = output.data.next_action.clone();
    let mut design = output.data.design;
    if cycle_envelope.is_some() && next_action.is_none() {
        return Err(AppError::with_details(
            AppErrorCode::Validation,
            "Active exploration cycle requires a typed PLAN next_action.",
            "Provider response must include next_action.action as BUILD, ASK, or STOP with exact sourceVersionId, hypothesis, changeScope, expectedEvidence, and budgetCost.".to_string(),
        ));
    }
    {
        let db = state.db.lock().await;
        record_cycle_model_event(
            &db,
            &ctx.thread_id,
            &engine,
            output.usage.as_ref(),
            model_started.elapsed().as_millis() as u64,
            None,
        )?;
    }

    // 4.2: emit content-free context telemetry (envelope shape plus provider
    // cache/input/output usage) through the existing profiler/session-activity
    // path. No new status bar or UI surface; the record is derived from the
    // shape-only envelope so it cannot leak prompt/source/references/image
    // bytes/API keys/headers/paths. Recorded after a successful dispatch so it
    // pairs section budget decisions with the provider's reported usage.
    state.record_context_telemetry(&generation_payload.envelope, output.usage.as_ref());

    if !question_mode {
        if framework_enabled {
            if engine_kind == crate::contracts::EngineKind::EckyIrV0 {
                design.macro_dialect = MacroDialect::EckyIrV0;
            } else if let Some(parsed) =
                crate::commands::design::derive_framework_controls(&design.macro_code)?
            {
                design.ui_spec = UiSpec {
                    fields: parsed.fields.clone(),
                };
                design.initial_params = parsed.params;
                design.macro_dialect = MacroDialect::CadFrameworkV1;
            } else {
                design.macro_dialect = MacroDialect::Legacy;
            }
        } else {
            design.macro_dialect = if source_language == crate::contracts::SourceLanguage::EckyIrV0
            {
                MacroDialect::EckyIrV0
            } else if source_language == crate::contracts::SourceLanguage::Build123d {
                MacroDialect::Build123d
            } else {
                MacroDialect::Legacy
            };
        }
        design.engine_kind = engine_kind;
        design.source_language = source_language;
        design.geometry_backend = geometry_backend;
    }

    if question_mode {
        design.interaction_mode = InteractionMode::Question;
        if let Some(previous) = &ctx.last_output {
            design.title = previous.title.clone();
            design.version_name = previous.version_name.clone();
            design.macro_code = previous.macro_code.clone();
            design.ui_spec = previous.ui_spec.clone();
            design.initial_params = previous.initial_params.clone();
            design.macro_dialect = previous.macro_dialect.clone();
            design.engine_kind = previous.engine_kind;
            design.source_language = previous.source_language;
            design.geometry_backend = previous.geometry_backend;
        }
        if design.version_name.trim().is_empty() {
            design.version_name = "Q&A".to_string();
        }
        if design.response.trim().is_empty() {
            design.response = "Question answered. Geometry unchanged.".to_string();
        }
    }

    if let Err(err) = validate_design_output(&design) {
        if !question_mode
            && design.macro_dialect == MacroDialect::Legacy
            && is_param_schema_mismatch(&err)
        {
            if let Some((ui_spec, initial_params, _heal_report)) = auto_heal_legacy_params(
                &design.macro_code,
                &design.ui_spec,
                &design.initial_params,
                ctx.last_output
                    .as_ref()
                    .map(|design| &design.initial_params),
            )? {
                design.ui_spec = ui_spec;
                design.initial_params = initial_params;
                validate_design_output(&design)?;
            } else {
                return Err(AppError::with_details(
                    AppErrorCode::Validation,
                    err.message,
                    "Legacy macro parameter mismatch could not be auto-healed because no dynamic params were parsed from the macro.".to_string(),
                ));
            }
        } else {
            return Err(err);
        }
    }

    Ok(GenerateOutput {
        design,
        thread_id: ctx.thread_id,
        message_id: Uuid::new_v4().to_string(),
        next_action,
        usage: output.usage,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn init_generation_attempt(
    thread_id: String,
    prompt: String,
    attachments: Option<Vec<Attachment>>,
    image_data: Option<String>,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<String> {
    init_generation_core(
        InitGenerationCoreInput {
            thread_id,
            prompt,
            attachments,
            image_data,
            classification_request_id: None,
        },
        state.inner(),
        &app,
    )
    .await
}

#[derive(Debug, Clone)]
pub(crate) struct InitGenerationCoreInput {
    pub thread_id: String,
    pub prompt: String,
    pub attachments: Option<Vec<Attachment>>,
    pub image_data: Option<String>,
    pub classification_request_id: Option<String>,
}

pub(crate) async fn init_generation_core(
    input: InitGenerationCoreInput,
    state: &AppState,
    app: &dyn crate::models::PathResolver,
) -> AppResult<String> {
    let InitGenerationCoreInput {
        thread_id,
        prompt,
        attachments,
        image_data,
        classification_request_id,
    } = input;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let assistant_message_id = Uuid::new_v4().to_string();
    let user_message_id = Uuid::new_v4().to_string();
    let configured_root = state.config.lock().unwrap().projects_root.clone();

    {
        let db = state.db.lock().await;
        ensure_generation_thread_binding(
            &db,
            app,
            configured_root.as_deref(),
            &thread_id,
            &prompt,
            now,
        )?;

        let attachment_images = collect_attachment_images(attachments.as_ref());
        let user_msg = Message {
            id: user_message_id.clone(),
            role: MessageRole::User,
            content: prompt.clone(),
            status: MessageStatus::Success,
            output: None,
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            structural_verification: None,
            agent_origin: None,
            image_data,
            visual_kind: None,
            attachment_images,
            timestamp: now,
        };
        db::add_message_checked(&db, &thread_id, &user_msg)
            .map_err(|err| AppError::persistence(err.to_string()))?;
        if let Some(request_id) = classification_request_id.as_deref() {
            crate::services::jev_classifications::bind_message(
                &db,
                &thread_id,
                "api",
                request_id,
                &user_message_id,
            )?;
            if let Some(app) = state.app_handle.lock().unwrap().clone() {
                use tauri::Emitter;
                let _ = app.emit(
                    "jev-classification-accepted",
                    serde_json::json!({"threadId": thread_id}),
                );
            }
        }
        persist_user_prompt_references(
            &db,
            &thread_id,
            &user_message_id,
            &prompt,
            attachments.as_ref(),
            now,
        )
        .map_err(AppError::persistence)?;

        let assistant_msg = Message {
            id: assistant_message_id.clone(),
            role: MessageRole::Assistant,
            content: "Generating...".to_string(),
            status: MessageStatus::Pending,
            output: None,
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            structural_verification: None,
            agent_origin: None,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
            timestamp: now + 1,
        };
        db::add_message_checked(&db, &thread_id, &assistant_msg)
            .map_err(|err| AppError::persistence(err.to_string()))?;
    }

    Ok(assistant_message_id)
}

/// Persist model-authored source before render or verification. The initial
/// pending row becomes the first immutable version. A later repair appends a
/// new version instead of overwriting the prior source.
#[tauri::command]
#[specta::specta]
pub async fn persist_generation_draft(
    message_id: String,
    design: DesignOutput,
    usage: Option<UsageSummary>,
    state: State<'_, AppState>,
) -> AppResult<String> {
    let db = state.db.lock().await;
    persist_generation_draft_in_db(&db, message_id, design, usage)
}

pub(crate) fn persist_generation_draft_in_db(
    db: &rusqlite::Connection,
    message_id: String,
    design: DesignOutput,
    usage: Option<UsageSummary>,
) -> AppResult<String> {
    // Reads normalize legacy projections before returning them. Normalize the
    // incoming snapshot before calculating identity too, otherwise a source
    // that infers Ecky dialect on read appears different from its own stored
    // legacy-labelled input and creates a duplicate immutable version.
    let design = crate::contracts::normalize_design_output(design);
    let thread_id = db::get_message_thread_id(&db, &message_id)
        .map_err(|error| AppError::persistence(error.to_string()))?
        .ok_or_else(|| {
            AppError::validation(format!("Generation message '{message_id}' was not found."))
        })?;
    let already_has_source: bool = db
        .query_row(
            "SELECT output IS NOT NULL FROM messages WHERE id = ?1 AND deleted_at IS NULL",
            [&message_id],
            |row| row.get(0),
        )
        .map_err(|error| AppError::persistence(error.to_string()))?;

    if !already_has_source {
        // `init_generation_core` creates a pending assistant placeholder before
        // the provider responds. If the provider returns the current source
        // unchanged, that placeholder must not become a second version. Hide
        // the non-version placeholder and keep the existing immutable head.
        let next_digest = crate::services::render_snapshot::canonical_version_input_digest(
            &design,
            &design.initial_params,
        )?;
        let latest_version = db::get_thread_latest_version(&db, &thread_id)
            .map_err(|error| AppError::persistence(error.to_string()))?;
        if let Some(existing) = latest_version {
            let identical = existing
                .output
                .as_ref()
                .map(|output| {
                    crate::services::render_snapshot::canonical_version_input_digest(
                        output,
                        &output.initial_params,
                    )
                })
                .transpose()?
                .is_some_and(|digest| digest == next_digest);
            let is_pending_placeholder: bool = db
                .query_row(
                    "SELECT role = 'assistant' AND status = 'pending'
                     FROM messages WHERE id = ?1 AND deleted_at IS NULL",
                    [&message_id],
                    |row| row.get(0),
                )
                .map_err(|error| AppError::persistence(error.to_string()))?;
            if identical && is_pending_placeholder && existing.id != message_id {
                db::update_message_status_and_output(
                    &db,
                    &message_id,
                    db::MessageStatusUpdate {
                        status: &MessageStatus::Discarded,
                        output: None,
                        usage: None,
                        artifact_bundle: None,
                        model_manifest: None,
                        structural_verification: None,
                        visual_kind: None,
                        content: None,
                    },
                )
                .map_err(|error| AppError::persistence(error.to_string()))?;
                return Ok(existing.id);
            }
        }
        db::update_message_status_and_output(
            &db,
            &message_id,
            db::MessageStatusUpdate {
                status: &MessageStatus::Working,
                output: Some(&design),
                usage: usage.as_ref(),
                artifact_bundle: None,
                model_manifest: None,
                structural_verification: None,
                visual_kind: None,
                content: Some(&design.response),
            },
        )
        .map_err(|error| AppError::persistence(error.to_string()))?;
        return Ok(message_id);
    }

    if let Some((existing, _)) = db::get_message_output_and_thread(&db, &message_id)
        .map_err(|error| AppError::persistence(error.to_string()))?
    {
        let existing_digest = crate::services::render_snapshot::canonical_version_input_digest(
            &existing,
            &existing.initial_params,
        )?;
        let next_digest = crate::services::render_snapshot::canonical_version_input_digest(
            &design,
            &design.initial_params,
        )?;
        if existing_digest == next_digest {
            return Ok(message_id);
        }
    }

    let next_id = Uuid::new_v4().to_string();
    let next_timestamp: u64 = db
        .query_row(
            "SELECT COALESCE(MAX(timestamp), 0) + 1 FROM messages WHERE thread_id = ?1",
            [&thread_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| AppError::persistence(error.to_string()))?
        .max(0) as u64;
    let message = Message {
        id: next_id.clone(),
        role: MessageRole::Assistant,
        content: design.response.clone(),
        status: MessageStatus::Working,
        output: Some(design),
        usage,
        artifact_bundle: None,
        model_manifest: None,
        structural_verification: None,
        agent_origin: None,
        image_data: None,
        visual_kind: None,
        attachment_images: Vec::new(),
        timestamp: next_timestamp,
    };
    db::add_legacy_message(&db, &thread_id, &message)
        .map_err(|error| AppError::persistence(error.to_string()))?;
    Ok(next_id)
}

fn ensure_generation_thread_binding(
    conn: &rusqlite::Connection,
    app: &dyn crate::models::PathResolver,
    configured_root: Option<&str>,
    thread_id: &str,
    prompt: &str,
    now: u64,
) -> AppResult<()> {
    if db::get_thread_title(conn, thread_id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .is_some()
    {
        return Ok(());
    }
    let traits = crate::generate_genie_traits();
    let chars: Vec<char> = prompt.chars().collect();
    let initial_title = if chars.len() > 30 {
        format!("{}...", chars[..27].iter().collect::<String>())
    } else {
        prompt.to_string()
    };
    db::create_or_update_thread(conn, thread_id, &initial_title, now, Some(&traits))
        .map_err(|err| AppError::persistence(err.to_string()))?;
    crate::thread_source_binding::bind_new_thread(
        app,
        conn,
        configured_root,
        thread_id,
        &initial_title,
    )?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
pub async fn finalize_generation_attempt(
    message_id: String,
    status: FinalizeStatus,
    design: Option<DesignOutput>,
    usage: Option<UsageSummary>,
    artifact_bundle: Option<ArtifactBundle>,
    model_manifest: Option<ModelManifest>,
    error_message: Option<String>,
    response_text: Option<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<()> {
    finalize_generation_core(
        FinalizeGenerationCoreInput {
            message_id,
            status,
            design,
            usage,
            artifact_bundle,
            model_manifest,
            error_message,
            response_text,
        },
        state.inner(),
        &app,
    )
    .await
}

#[derive(Debug, Clone)]
pub(crate) struct FinalizeGenerationCoreInput {
    pub message_id: String,
    pub status: FinalizeStatus,
    pub design: Option<DesignOutput>,
    pub usage: Option<UsageSummary>,
    pub artifact_bundle: Option<ArtifactBundle>,
    pub model_manifest: Option<ModelManifest>,
    pub error_message: Option<String>,
    pub response_text: Option<String>,
}

pub(crate) async fn finalize_generation_core(
    input: FinalizeGenerationCoreInput,
    state: &AppState,
    app: &dyn crate::models::PathResolver,
) -> AppResult<()> {
    let FinalizeGenerationCoreInput {
        message_id,
        status,
        design,
        usage,
        artifact_bundle,
        model_manifest,
        error_message,
        response_text,
    } = input;
    if let Some(design) = design.as_ref() {
        validate_design_output(design)?;
    }

    let configured_root = state.config.lock().unwrap().projects_root.clone();
    let db = state.db.lock().await;
    let thread_id = db::get_message_thread_id(&db, &message_id)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    let content = match status {
        FinalizeStatus::Success => {
            if let Some(design) = &design {
                if design.response.trim().is_empty() {
                    Some("Synthesized design output.".to_string())
                } else {
                    Some(design.response.clone())
                }
            } else {
                response_text.clone()
            }
        }
        FinalizeStatus::Error | FinalizeStatus::Discarded => error_message.clone(),
    };

    db::update_message_status_and_output(
        &db,
        &message_id,
        db::MessageStatusUpdate {
            status: &match status {
                FinalizeStatus::Success => MessageStatus::Success,
                FinalizeStatus::Error => MessageStatus::Error,
                FinalizeStatus::Discarded => MessageStatus::Discarded,
            },
            output: design.as_ref(),
            usage: usage.as_ref(),
            artifact_bundle: artifact_bundle.as_ref(),
            model_manifest: model_manifest.as_ref(),
            structural_verification: None,
            visual_kind: None,
            content: content.as_deref(),
        },
    )
    .map_err(|err| AppError::persistence(err.to_string()))?;

    if status == FinalizeStatus::Success {
        if let Some(thread_id) = thread_id {
            let title = design
                .as_ref()
                .map(|item| item.title.clone())
                .or_else(|| {
                    response_text.clone().map(|text| {
                        if text.len() > 30 {
                            format!("{}...", &text[..27])
                        } else {
                            text
                        }
                    })
                })
                .unwrap_or_else(|| "Question Session".to_string());
            let _ = persist_thread_summary(&db, &thread_id, &title);
            let appended_model_id = artifact_bundle
                .as_ref()
                .map(|bundle| bundle.model_id.clone())
                .or_else(|| {
                    model_manifest
                        .as_ref()
                        .map(|manifest| manifest.model_id.clone())
                });
            let appended_source = design
                .as_ref()
                .map(|design| (design.title.clone(), design.macro_code.clone()));

            if design.is_some() || artifact_bundle.is_some() || model_manifest.is_some() {
                let snapshot = build_saved_version_snapshot(
                    design,
                    thread_id.clone(),
                    message_id.clone(),
                    artifact_bundle,
                    model_manifest,
                    None,
                );
                {
                    let mut last = state.last_snapshot.lock().unwrap();
                    *last = Some(snapshot.clone());
                }
                write_last_snapshot(app, Some(&snapshot));
            }
            if let Some((design_title, macro_code)) = appended_source {
                crate::thread_source_binding::refresh_on_version_append(
                    app,
                    &db,
                    configured_root.as_deref(),
                    &thread_id,
                    &design_title,
                    &macro_code,
                    &message_id,
                    appended_model_id.as_deref(),
                    Some(&message_id),
                )?;
            }
        }
    }

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn persist_structural_verification(
    message_id: String,
    structural_verification: StructuralVerificationResult,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let db = state.db.lock().await;
    persist_structural_verification_core(&db, &message_id, &structural_verification)
}

pub(crate) fn persist_structural_verification_core(
    db: &rusqlite::Connection,
    message_id: &str,
    structural_verification: &StructuralVerificationResult,
) -> AppResult<()> {
    db::update_message_structural_verification(db, message_id, Some(structural_verification))
        .map_err(|err| AppError::persistence(err.to_string()))
}

#[tauri::command]
#[specta::specta]
pub async fn classify_intent(
    prompt: String,
    thread_id: Option<String>,
    context: Option<String>,
    image_data: Option<String>,
    attachments: Option<Vec<Attachment>>,
    state: State<'_, AppState>,
) -> AppResult<IntentDecision> {
    classify_intent_core(
        ClassifyIntentCoreInput {
            prompt,
            thread_id,
            context,
            image_data,
            attachments,
        },
        state.inner(),
    )
    .await
}

#[derive(Debug, Clone)]
pub(crate) struct ClassifyIntentCoreInput {
    pub prompt: String,
    pub thread_id: Option<String>,
    pub context: Option<String>,
    pub image_data: Option<String>,
    pub attachments: Option<Vec<Attachment>>,
}

pub(crate) async fn classify_intent_core(
    input: ClassifyIntentCoreInput,
    state: &AppState,
) -> AppResult<IntentDecision> {
    classify_intent_with_route_core(input, state)
        .await
        .map(|(decision, _)| decision)
}

pub(crate) async fn classify_intent_with_route_core(
    input: ClassifyIntentCoreInput,
    state: &AppState,
) -> AppResult<(IntentDecision, Option<crate::jev_classifier::AcceptedRoute>)> {
    classify_intent_with_route_with_classifier(input, state, None).await
}

async fn classify_intent_with_route_with_classifier(
    input: ClassifyIntentCoreInput,
    state: &AppState,
    classifier: Option<&dyn crate::jev_classifier::TurnClassifier>,
) -> AppResult<(IntentDecision, Option<crate::jev_classifier::AcceptedRoute>)> {
    let ClassifyIntentCoreInput {
        prompt,
        thread_id,
        context,
        image_data,
        attachments,
    } = input;
    let engine = selected_engine_core(state)?;
    let explicit_question_only = crate::is_explicit_question_only_request(&prompt);
    let backend_context = if thread_id.is_some() {
        let ctx = {
            let db = state.db.lock().await;
            crate::context::assemble_context(&db, thread_id, None, None)
        };
        // 3.4: route classification through its own compact typed envelope (8K
        // ceiling; design digest + latest dialogue + frontend snapshot; pinned
        // references only with attachment/reference intent). Full source, params,
        // and authoring policy never enter the classifier projection. On envelope
        // overflow the classifier falls back to a request-only context rather
        // than dispatching a lossy one.
        let include_references = attachments
            .as_ref()
            .map(|items| !items.is_empty())
            .unwrap_or(false)
            || !ctx.pinned_references.trim().is_empty();
        match crate::context::assemble_classifier_context(
            &ctx,
            context.as_deref(),
            include_references,
        ) {
            Ok(rendered) if !rendered.trim().is_empty() => Some(rendered),
            _ => None,
        }
    } else {
        context
    };

    let prompt =
        if let Some(notes) = build_visual_input_notes(image_data.as_ref(), attachments.as_ref()) {
            format!("{}\n\n{}", prompt, notes)
        } else {
            prompt
        };
    let images = prepare_images(image_data, attachments.clone());
    let jev_config = state.config.lock().unwrap().jev_classifier.clone();
    if jev_config.enabled {
        let attachment_modalities = attachments
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|attachment| crate::jev_classifier::AttachmentModality {
                kind: attachment.kind.as_str().to_owned(),
                explanation: attachment.explanation.clone(),
            })
            .chain(
                images
                    .iter()
                    .map(|_| crate::jev_classifier::AttachmentModality {
                        kind: "image".into(),
                        explanation: "inline image supplied with current request".into(),
                    }),
            )
            .collect();
        let request = crate::jev_classifier::ClassifierRequest::bounded(
            &prompt,
            std::iter::empty(),
            backend_context.as_deref().unwrap_or_default(),
            &format!(
                "API engine {}; configured model ceiling {}",
                engine.provider,
                if engine.model.trim().is_empty() {
                    "provider default"
                } else {
                    engine.model.trim()
                }
            ),
            attachment_modalities,
            Vec::new(),
        );
        let mut route = match classifier {
            Some(classifier) => classifier.classify(request).await?,
            None => {
                crate::jev_classifier::classify_configured_request(&jev_config, request).await?
            }
        };
        if route.model.is_some() {
            return Err(AppError::validation(
                "Jev returned an unverified model override for this API engine.",
            ));
        }
        route.model_reason = format!(
            "configured API model retained ({}); no verified comparable discovered model catalog",
            engine.model
        );
        let answer_only = !matches!(
            route.intent,
            crate::provider_turn::ProviderTurnIntent::Modify
        );
        return Ok((
            IntentDecision {
                intent_mode: if answer_only { "question" } else { "design" }.into(),
                confidence: route.action_confidence as f32,
                response: format!("Jev selected {} contract.", route.intent.as_str()),
                final_response: None,
                usage: None,
            },
            Some(route),
        ));
    }
    match llm::classify_intent(&engine, &prompt, backend_context.as_deref(), images).await {
        Ok(classification) => {
            let llm::IntentClassification {
                intent,
                confidence,
                response,
                final_response,
            } = classification.data;
            let final_response = if explicit_question_only {
                final_response.clone().or_else(|| Some(response.clone()))
            } else {
                final_response.clone()
            };
            Ok((
                IntentDecision {
                    intent_mode: if explicit_question_only {
                        "question".to_string()
                    } else {
                        intent
                    },
                    confidence,
                    response,
                    final_response,
                    usage: classification.usage,
                },
                None,
            ))
        }
        Err(_) => Ok((fallback_intent(&prompt), None)),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn verify_render(
    original_prompt: String,
    screenshots: Vec<String>,
    reference_image_paths: Vec<String>,
    structural_summary: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<crate::contracts::VisualVerificationResult> {
    let engine = selected_engine(&state)?;

    // Convert reference image file paths to data URLs
    let reference_images: Vec<String> = reference_image_paths
        .iter()
        .filter_map(|path| {
            let bytes = fs::read(path).ok()?;
            let b64 = general_purpose::STANDARD.encode(bytes);
            let ext = path.split('.').next_back().unwrap_or("png").to_lowercase();
            let mime = match ext.as_str() {
                "jpg" | "jpeg" => "image/jpeg",
                "webp" => "image/webp",
                "svg" => "image/svg+xml",
                _ => "image/png",
            };
            Some(format!("data:{};base64,{}", mime, b64))
        })
        .collect();

    llm::verify_render(
        &engine,
        &original_prompt,
        screenshots,
        reference_images,
        structural_summary.as_deref(),
    )
    .await
    .map(|outcome| outcome.data)
    .map_err(|raw_body| {
        AppError::with_details(
            AppErrorCode::Provider,
            "Vision verification LLM call failed.",
            raw_body,
        )
    })
}

#[tauri::command]
#[specta::specta]
pub async fn verify_generated_model(
    model_id: String,
    #[allow(unused_variables)] original_prompt: String,
    app: AppHandle,
) -> AppResult<crate::contracts::StructuralVerificationResult> {
    let bundle = crate::model_runtime::read_artifact_bundle(&app, &model_id)?;
    let manifest = crate::model_runtime::read_model_manifest(&app, &model_id)?;
    Ok(
        crate::services::author_verification_foundation::verify_structure_with_author_verification(
            &bundle, &manifest,
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        build123d_guide_text, build123d_python_guide_text, design_system_prompt,
        ecky_ir_v0_guide_text, freecad_guide_text, prepend_follow_up_context,
        resolve_generation_engine_kind, resolve_generation_geometry_backend,
        resolve_generation_source_language, should_use_framework_for_generation,
    };
    use crate::context::PromptContext;
    use crate::contracts::{Config, McpConfig};
    use crate::contracts::{
        DesignOutput, EngineKind, GeometryBackend, InteractionMode, MacroDialect, SourceLanguage,
        UiSpec,
    };
    use crate::models::AppState;
    use std::path::PathBuf;

    fn test_db_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ecky-generation-{}-{}", name, uuid::Uuid::new_v4()))
    }

    fn test_config() -> Config {
        Config {
            engines: Vec::new(),
            selected_engine_id: String::new(),
            freecad_cmd: String::new(),
            cad_text_font_path: String::new(),
            freecad_library_roots: Vec::new(),
            assets: Vec::new(),
            microwave: None,
            voice: crate::contracts::VoiceConfig::default(),
            mcp: McpConfig::default(),
            fem_compute: crate::contracts::FemComputeConfig::default(),
            has_seen_onboarding: true,
            connection_type: None,
            provider_models: crate::contracts::ProviderModels::default(),
            jev_classifier: Default::default(),
            default_engine_kind: EngineKind::Freecad,
            default_source_language: SourceLanguage::LegacyPython,
            default_geometry_backend: GeometryBackend::Freecad,
            max_generation_attempts: 3,
            max_verify_attempts: 0,
            projects_root: None,
        }
    }

    #[tokio::test]
    async fn api_jev_failure_stops_before_selected_engine_dispatch() {
        struct FailingJev;
        impl crate::jev_classifier::TurnClassifier for FailingJev {
            fn classify<'a>(
                &'a self,
                _request: crate::jev_classifier::ClassifierRequest,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = crate::contracts::AppResult<
                                crate::jev_classifier::AcceptedRoute,
                            >,
                        > + Send
                        + 'a,
                >,
            > {
                Box::pin(async {
                    Err(crate::contracts::AppError::provider(
                        "Jev fixture denied route",
                    ))
                })
            }
        }

        let mut config = test_config();
        config.engines.push(crate::contracts::Engine {
            id: "api-engine".into(),
            name: "API engine".into(),
            provider: "openai".into(),
            api_key: "fixture-provider-token".into(),
            model: "gpt-5.6-sol".into(),
            light_model: String::new(),
            base_url: "http://127.0.0.1:1/v1".into(),
            enabled: true,
            vision_overrides: Default::default(),
        });
        config.selected_engine_id = "api-engine".into();
        config.jev_classifier.enabled = true;
        config.jev_classifier.api_key = "fixture-jev-token".into();
        let conn = crate::db::init_db(&test_db_path("api-jev-failure")).expect("db");
        let state = AppState::new(config, None, conn);
        let error = super::classify_intent_with_route_with_classifier(
            super::ClassifyIntentCoreInput {
                prompt: "Explain bracket dimensions".into(),
                thread_id: None,
                context: None,
                image_data: None,
                attachments: None,
            },
            &state,
            Some(&FailingJev),
        )
        .await
        .expect_err("Jev failure blocks provider route");
        assert_eq!(error.message, "Jev fixture denied route");
    }

    #[tokio::test]
    async fn api_jev_route_retains_configured_model_and_rejects_unverified_override() {
        let mut config = test_config();
        config.engines.push(crate::contracts::Engine {
            id: "api-engine".into(),
            name: "API engine".into(),
            provider: "openai".into(),
            api_key: "fixture-provider-token".into(),
            model: "gpt-5.6-sol".into(),
            light_model: String::new(),
            base_url: "http://127.0.0.1:1/v1".into(),
            enabled: true,
            vision_overrides: Default::default(),
        });
        config.selected_engine_id = "api-engine".into();
        config.jev_classifier.enabled = true;
        config.jev_classifier.api_key = "fixture-jev-token".into();
        let conn = crate::db::init_db(&test_db_path("api-jev-model")).expect("db");
        let state = AppState::new(config, None, conn);
        let input = super::ClassifyIntentCoreInput {
            prompt: "Explain bracket dimensions".into(),
            thread_id: None,
            context: None,
            image_data: None,
            attachments: None,
        };
        let retained = crate::jev_classifier::MockJevClassifier::fixed(
            crate::jev_classifier::AcceptedRoute::test_route(
                crate::provider_turn::ProviderTurnIntent::Answer,
                None,
                false,
            ),
        );
        let (_, route) = super::classify_intent_with_route_with_classifier(
            input.clone(),
            &state,
            Some(&retained),
        )
        .await
        .expect("valid route");
        assert!(route
            .unwrap()
            .model_reason
            .contains("configured API model retained"));

        let unverified = crate::jev_classifier::MockJevClassifier::fixed(
            crate::jev_classifier::AcceptedRoute::test_route(
                crate::provider_turn::ProviderTurnIntent::Answer,
                Some("gpt-5.6-luna".into()),
                false,
            ),
        );
        let error =
            super::classify_intent_with_route_with_classifier(input, &state, Some(&unverified))
                .await
                .expect_err("unverified API override");
        assert!(error.message.contains("unverified model override"));
    }

    fn prompt_context_with_last_output(macro_dialect: MacroDialect) -> PromptContext {
        PromptContext {
            thread_id: "thread-1".to_string(),
            thread_title: "Thread".to_string(),
            summary: String::new(),
            recent_dialogue: String::new(),
            pinned_references: String::new(),
            available_assets: String::new(),
            last_output: Some(DesignOutput {
                title: "Design".to_string(),
                version_name: "v1".to_string(),
                response: String::new(),
                interaction_mode: InteractionMode::Design,
                macro_code: "import FreeCAD".to_string(),
                macro_dialect: macro_dialect.clone(),
                engine_kind: if macro_dialect == MacroDialect::EckyIrV0 {
                    crate::contracts::EngineKind::EckyIrV0
                } else {
                    crate::contracts::EngineKind::Freecad
                },
                geometry_backend: if macro_dialect == MacroDialect::EckyIrV0 {
                    crate::contracts::GeometryBackend::EckyRust
                } else {
                    crate::contracts::GeometryBackend::Freecad
                },
                source_language: if macro_dialect == MacroDialect::EckyIrV0 {
                    crate::contracts::SourceLanguage::EckyIrV0
                } else {
                    crate::contracts::SourceLanguage::LegacyPython
                },
                ui_spec: UiSpec { fields: Vec::new() },
                initial_params: Default::default(),
                post_processing: None,
            }),
            design_digest: "Current working snapshot\nDesign [v1]".to_string(),
            artifact_digest: String::new(),
        }
    }

    #[test]
    fn prepend_follow_up_context_is_noop_when_missing() {
        let prompt = "CURRENT DESIGN CONTEXT\n...".to_string();
        let result = prepend_follow_up_context(prompt.clone(), None);
        assert_eq!(result, prompt);
    }

    #[test]
    fn prepend_follow_up_context_adds_authoritative_block() {
        let result = prepend_follow_up_context(
            "CURRENT DESIGN CONTEXT\n...".to_string(),
            Some("Which side?"),
        );
        assert!(result.contains("FOLLOW-UP CONTEXT (AUTHORITATIVE)"));
        assert!(result.contains("\"Which side?\""));
        assert!(result.contains("Do not repeat the same clarification question"));
        assert!(result.ends_with("CURRENT DESIGN CONTEXT\n..."));
    }

    #[test]
    fn framework_contract_stays_enabled_even_for_legacy_threads() {
        let ctx = prompt_context_with_last_output(MacroDialect::Legacy);
        assert!(should_use_framework_for_generation(&ctx));
    }

    #[test]
    fn framework_contract_is_omitted_for_ecky_and_build123d_contexts() {
        // 3.3: the FreeCAD CAD-SDK framework contract applies only to FreeCAD-
        // based authoring. Ecky and build123d have their own language surfaces.
        let ecky = PromptContext {
            last_output: Some(DesignOutput {
                title: "D".to_string(),
                version_name: "v1".to_string(),
                response: String::new(),
                interaction_mode: InteractionMode::Design,
                macro_code: "(model (part body (box 1 1 1)))".to_string(),
                macro_dialect: MacroDialect::EckyIrV0,
                engine_kind: EngineKind::EckyIrV0,
                source_language: SourceLanguage::EckyIrV0,
                geometry_backend: GeometryBackend::EckyRust,
                ui_spec: UiSpec { fields: Vec::new() },
                initial_params: Default::default(),
                post_processing: None,
            }),
            ..prompt_context_with_last_output(MacroDialect::EckyIrV0)
        };
        let build123d = PromptContext {
            last_output: Some(DesignOutput {
                title: "D".to_string(),
                version_name: "v1".to_string(),
                response: String::new(),
                interaction_mode: InteractionMode::Design,
                macro_code: "from build123d import *".to_string(),
                macro_dialect: MacroDialect::Build123d,
                engine_kind: EngineKind::Freecad,
                source_language: SourceLanguage::Build123d,
                geometry_backend: GeometryBackend::Build123d,
                ui_spec: UiSpec { fields: Vec::new() },
                initial_params: Default::default(),
                post_processing: None,
            }),
            ..prompt_context_with_last_output(MacroDialect::Build123d)
        };
        assert!(!should_use_framework_for_generation(&ecky));
        assert!(!should_use_framework_for_generation(&build123d));
    }

    #[test]
    fn design_system_prompt_includes_framework_only_when_applicable() {
        // 3.2/3.3: the static system prefix carries the output contract, the
        // language body, and (only when provided) the CAD-framework rules.
        let without =
            design_system_prompt(SourceLanguage::LegacyPython, GeometryBackend::Freecad, None);
        // The output contract references the framework block by name, but the
        // actual contract body / `(AUTHORITATIVE):` header must be absent when no
        // framework is provided.
        assert!(!without.contains("ACTUAL CURRENT CAD FRAMEWORK (AUTHORITATIVE):"));
        assert!(!without.contains("CAD-SDK-CONTRACT-BODY"));
        assert!(without.contains("TARGET LANGUAGE GUIDE"));

        let with = design_system_prompt(
            SourceLanguage::LegacyPython,
            GeometryBackend::Freecad,
            Some("CAD-SDK-CONTRACT-BODY"),
        );
        assert!(with.contains("ACTUAL CURRENT CAD FRAMEWORK (AUTHORITATIVE):"));
        assert!(with.contains("CAD-SDK-CONTRACT-BODY"));
        // The framework appears exactly once.
        assert_eq!(with.matches("CAD-SDK-CONTRACT-BODY").count(), 1);
    }

    #[test]
    fn api_system_prompt_uses_shared_language_builder_exactly_once() {
        // 3.1: the API system prompt consumes the single-source language builder
        // (`agent_language_reference`) exactly once; no second language copy is
        // introduced into the assembled generation payload.
        let prompt =
            design_system_prompt(SourceLanguage::EckyIrV0, GeometryBackend::EckyRust, None);
        let body = crate::agent_prompt::canonical_agent_reference();
        assert_eq!(
            prompt.matches(body).count(),
            1,
            "shared language body must appear exactly once in the system prompt"
        );
    }

    #[test]
    fn api_system_prompt_contains_exploration_invariants_exactly_once() {
        let prompt =
            design_system_prompt(SourceLanguage::EckyIrV0, GeometryBackend::EckyRust, None);
        assert_eq!(
            prompt
                .matches(crate::exploration_prompt::STATIC_GUIDANCE)
                .count(),
            1
        );
        assert_eq!(prompt.matches("PLAN, BUILD, VERIFY, DECIDE").count(), 1);
    }

    #[test]
    fn api_system_prompt_contains_one_transient_typed_next_action_contract() {
        let prompt =
            design_system_prompt(SourceLanguage::EckyIrV0, GeometryBackend::EckyRust, None);
        assert_eq!(prompt.matches("\"next_action\": {").count(), 1);
        assert!(prompt.contains("\"action\": \"BUILD\"|\"ASK\"|\"STOP\""));
        assert!(prompt.contains("\"source_version_id\": string"));
        assert!(prompt.contains("\"hypothesis\": string"));
        assert!(prompt.contains("\"change_scope\": string"));
        assert!(prompt.contains("\"expected_evidence\": string"));
        assert!(prompt.contains("\"budget_cost\": non-negative integer"));
        assert!(prompt.contains("transient controller input"));
        assert!(prompt.contains("same authoring response"));
        assert!(prompt.contains("ASK and STOP still return the complete existing design object"));
    }

    #[test]
    #[ignore = "superseded by canonical agent-language source contracts"]
    fn guide_texts_use_file_hints_and_backend_truth() {
        let build123d = build123d_guide_text();
        assert!(build123d.contains("Current fileExtension: `.ecky`."));
        assert!(build123d.contains("Current sourceLanguage: `ecky`."));
        assert!(build123d.contains("Target geometryBackend: `build123d`."));
        assert!(build123d.contains("Return canonical Ecky source in `macro_code`."));
        assert!(build123d.contains("Start every renderable answer with `(model ...)`."));
        assert!(build123d.contains("PROGRESSIVE ECKY EXAMPLES"));
        assert!(build123d.contains("1. First solid"));
        assert!(build123d.contains("(sphere 10)"));
        assert!(build123d.contains("2. Sketch then extrude"));
        assert!(build123d.contains("(extrude (rounded-rect 70 42 5) 4)"));
        assert!(build123d.contains("3. Sketch with a hole"));
        assert!(build123d.contains("(profile :outer (rounded-rect 70 42 5)"));
        assert!(build123d.contains("4. Parameters, named stages, and cuts"));
        assert!(build123d.contains("(params"));
        assert!(build123d.contains("(build"));
        assert!(build123d.contains("(difference blank left-hole right-hole)"));
        assert!(build123d.contains("5. Repetition instead of copy-paste"));
        assert!(build123d.contains("(repeat-union i 5"));
        assert!(build123d.contains("6. Final-pattern model"));
        assert!(build123d.contains("helical-ridge"));
        assert!(build123d.contains("clip-box"));
        assert!(build123d.contains("READING ORDER FOR GENERATED CODE"));
        assert!(build123d.contains("Use `let*` when later bindings depend on earlier ones"));
        assert!(build123d.contains("Use `map`, `range`, `repeat-union`, and `repeat-compound`"));
        assert!(build123d.contains("Name fit-critical bindings"));
        assert!(build123d.contains("API MODE ONE-PROMPT WORKFLOW"));
        assert!(build123d.contains("API mode cannot call MCP tools"));
        assert!(build123d.contains("do not claim you ran them"));
        assert!(build123d.contains("VERIFY CLAUSES"));
        assert!(build123d.contains("(verify"));
        assert!(build123d.contains("Clause grammar"));
        assert!(build123d
            .contains("Metric namespaces: `manifest`, `stl`, `clearance`, `selector`, `relation`"));
        assert!(build123d.contains("(manifest has-model-stl)"));
        assert!(build123d.contains("(manifest part-count)"));
        assert!(build123d.contains("(stl non-manifold-edge-count)"));
        assert!(build123d.contains("(stl triangle-count)"));
        assert!(build123d.contains("(stl bed-contact-area-ratio [part-id])"));
        assert!(build123d.contains("(stl bed-contact-x-span-ratio [part-id])"));
        assert!(build123d.contains("(stl bed-contact-y-span-ratio [part-id])"));
        assert!(build123d.contains("(clearance min-distance selector-a selector-b)"));
        assert!(build123d.contains("(selector axis selector)"));
        assert!(build123d.contains("(selector extent-x selector)"));
        assert!(build123d.contains("(selector center-z selector)"));
        assert!(build123d.contains("(relation axis-angle selector-a selector-b)"));
        assert!(build123d.contains("(relation center-delta-x selector-a selector-b)"));
        assert!(build123d.contains("(relation center-delta-y selector-a selector-b)"));
        assert!(build123d.contains("(relation center-delta-z selector-a selector-b)"));
        assert!(build123d.contains("Axis returns text: `x`, `y`, or `z`"));
        assert!(build123d.contains("Axis angle is unsigned degrees"));
        assert!(build123d.contains("Operators: `=`, `!=`, `>`, `>=`, `<`, `<=`"));
        assert!(build123d.contains("(metric check (manifest has-model-stl))"));
        assert!(build123d.contains("(expect check (= true))"));
        assert!(build123d.contains("(metric bad_edges (stl non-manifold-edge-count))"));
        assert!(build123d.contains("(expect bad_edges (= 0))"));
        assert!(build123d.contains("(metric gap (clearance min-distance body lid))"));
        assert!(build123d.contains("(expect gap (>= 0.3))"));
        assert!(build123d.contains("(metric parts (manifest part-count))"));
        assert!(build123d.contains("(metric axis (selector axis joint_tongue))"));
        assert!(build123d.contains("(expect axis (= \"y\"))"));
        assert!(build123d.contains("(metric width (selector extent-x joint_tongue))"));
        assert!(build123d.contains("(expect width (>= 11.8))"));
        assert!(build123d.contains("(metric angle (relation axis-angle tube_axis joint_tongue))"));
        assert!(build123d.contains("(expect angle (>= 85))"));
        assert!(!build123d.contains("(min_wall_thickness"));
        assert!(build123d.contains("Do not remove or weaken existing `(verify ...)` clauses"));
        assert!(build123d.contains("`offset-rounded`"));
        assert!(build123d.contains("`grid-array`"));
        assert!(build123d.contains("`arc-array`"));
        assert!(build123d.contains("`deg->rad`"));
        assert!(build123d.contains("`rad->deg`"));
        assert!(build123d.contains("never emit Python source for `.ecky` requests"));
        assert!(!build123d.contains("`wall-pattern`"));
        assert!(!build123d.contains("`schwarz-p`"));
        assert!(!build123d.contains("`schwarz-d`"));
        assert!(!build123d.contains("`diamond-field`"));
        assert!(!build123d.contains("`neovius`"));
        assert!(!build123d.contains("`attractor-field`"));

        let ecky = ecky_ir_v0_guide_text(crate::contracts::GeometryBackend::EckyRust);
        assert!(ecky.contains("Current fileExtension: `.ecky`."));
        assert!(ecky.contains("Current sourceLanguage: `ecky`."));
        assert!(ecky.contains("never from thread metadata"));
        assert!(ecky.contains("renders through EckyRust CAD VM"));
        assert!(ecky.contains("Do not promise STEP"));
        assert!(ecky.contains("ArtifactBundle.exportArtifacts"));
        assert!(ecky.contains("PROGRESSIVE ECKY EXAMPLES"));
        assert!(
            ecky.contains("write direct `(verify ...)` clauses under the top-level `(model ...)`")
        );
        assert!(ecky.contains("red first render is expected repair input"));
        assert!(ecky.contains("(sphere 10)"));
        assert!(ecky.contains("(extrude (rounded-rect 70 42 5) 4)"));
        assert!(ecky.contains("(difference blank left-hole right-hole)"));
        assert!(ecky.contains("(repeat-union i 5"));
        assert!(ecky.contains("helical-ridge"));
        assert!(ecky.contains("clip-box"));
        assert!(ecky.contains("`wall-pattern`"));
        assert!(ecky.contains("`cellular`"));
        assert!(ecky.contains("`gyroid`"));
        assert!(ecky.contains("`schwarz-p`"));
        assert!(ecky.contains("`schwarz-d`"));
        assert!(ecky.contains("`diamond-field`"));
        assert!(ecky.contains("`neovius`"));
        assert!(ecky.contains("`attractor-field`"));

        let freecad = freecad_guide_text();
        assert!(freecad.contains("Current fileExtension: `.ecky`."));
        assert!(freecad.contains("Target geometryBackend: `freecad`."));
        assert!(freecad.contains("Return canonical Ecky source in `macro_code`."));
        assert!(freecad.contains("This is still `.ecky` source"));
        assert!(freecad.contains("never emit Python source for `.ecky` requests"));
        assert!(freecad.contains("PROGRESSIVE ECKY EXAMPLES"));
        assert!(freecad.contains("(sphere 10)"));
        assert!(freecad.contains("Sketch then extrude"));
        assert!(freecad.contains("Final-pattern model"));
        assert!(freecad.contains("`grid-array`"));
        assert!(freecad.contains("`arc-array`"));
        assert!(!freecad.contains("`wall-pattern`"));
        assert!(!freecad.contains("`schwarz-p`"));
        assert!(!freecad.contains("`schwarz-d`"));
        assert!(!freecad.contains("`diamond-field`"));
        assert!(!freecad.contains("`neovius`"));
        assert!(!freecad.contains("`attractor-field`"));

        let raw_python = build123d_python_guide_text();
        assert!(raw_python.contains("Current fileExtension: `.py`."));
        assert!(raw_python.contains("Current sourceLanguage: `build123d`."));
        assert!(raw_python.contains("Return canonical `build123d` source in `macro_code`."));
    }

    #[test]
    fn guide_lists_only_manifest_cad_ops_for_backend_specific_surface() {
        let freecad = freecad_guide_text();
        let mesh = ecky_ir_v0_guide_text(crate::contracts::GeometryBackend::EckyRust);

        for op in crate::ecky_language_surface::CAD_OPS_PORTABLE {
            assert!(freecad.contains(&format!("`{op}`")), "freecad missing {op}");
            assert!(mesh.contains(&format!("`{op}`")), "mesh missing {op}");
            assert!(
                crate::ecky_scheme::cad::MODULE.exports.contains(op),
                "manifest op not exported: {op}"
            );
        }
        assert!(!freecad.contains("`align`"));
        assert!(!freecad.contains("`wall-pattern`"));
        assert!(mesh.contains("`wall-pattern`"));
    }

    #[tokio::test]
    async fn resolver_uses_config_authoring_context_without_version_context() {
        let conn = crate::db::init_db(&test_db_path("thread-authoring-precedence")).expect("db");
        let mut config = test_config();
        config.default_engine_kind = EngineKind::EckyIrV0;
        config.default_source_language = SourceLanguage::EckyIrV0;
        config.default_geometry_backend = GeometryBackend::Build123d;
        let state = AppState::new(config, None, conn);
        let thread_id = "thread-authoring";

        {
            let db = state.db.lock().await;
            crate::db::create_or_update_thread(&db, thread_id, "Thread", 100, None)
                .expect("thread");
        }

        let engine_kind = resolve_generation_engine_kind(&state, Some(thread_id), None, None, None)
            .await
            .expect("engine kind");
        let source_language =
            resolve_generation_source_language(&state, Some(thread_id), None, None, None)
                .await
                .expect("source language");
        let geometry_backend =
            resolve_generation_geometry_backend(&state, Some(thread_id), None, None, None)
                .await
                .expect("geometry backend");

        assert_eq!(engine_kind, EngineKind::EckyIrV0);
        assert_eq!(source_language, SourceLanguage::EckyIrV0);
        assert_eq!(geometry_backend, GeometryBackend::Build123d);
    }

    #[tokio::test]
    async fn resolver_uses_version_metadata_before_config_defaults() {
        let conn =
            crate::db::init_db(&test_db_path("thread-authoring-before-version")).expect("db");
        let state = AppState::new(test_config(), None, conn);
        let thread_id = "thread-authoring";
        let stale_design = prompt_context_with_last_output(MacroDialect::Legacy)
            .last_output
            .expect("last output");

        {
            let db = state.db.lock().await;
            crate::db::create_or_update_thread(&db, thread_id, "Thread", 100, None)
                .expect("thread");
        }

        let engine_kind = resolve_generation_engine_kind(
            &state,
            Some(thread_id),
            None,
            Some(&stale_design),
            Some(&stale_design),
        )
        .await
        .expect("engine kind");
        let source_language = resolve_generation_source_language(
            &state,
            Some(thread_id),
            None,
            Some(&stale_design),
            Some(&stale_design),
        )
        .await
        .expect("source language");
        let geometry_backend = resolve_generation_geometry_backend(
            &state,
            Some(thread_id),
            None,
            Some(&stale_design),
            Some(&stale_design),
        )
        .await
        .expect("geometry backend");

        assert_eq!(engine_kind, stale_design.engine_kind);
        assert_eq!(source_language, stale_design.source_language);
        assert_eq!(geometry_backend, stale_design.geometry_backend);
    }
}

// OpenSpec `agent-context-budgeting`, section 1 — OUTER RED tests (now GREEN).
//
// These pin the provider/API envelope contract at the generation-assembly
// seam. They started RED before the section-3 wiring landed and are now GREEN:
// one technical-policy copy, a stable system prefix with a variable user tail,
// total-size metadata covering the full user content, and a pre-dispatch
// mandatory-overflow failure. Sub-behaviours that already held are kept as
// green evidence inside the same test. No telemetry or frontend changes here —
// only the smallest protocol/payload assertions at the assembly seam.
#[cfg(test)]
mod agent_context_budget_outer_red {
    use super::{
        design_system_prompt, ensure_generation_thread_binding, persist_generation_draft_in_db,
    };
    use crate::context::{assemble_generation_payload, PromptContext, ResolvedAuthoringContext};
    use crate::context_envelope::GENERATION_CEILING_CHARS;
    use crate::contracts::{
        DesignOutput, EngineKind, GeometryBackend, InteractionMode, MacroDialect, Message,
        MessageRole, MessageStatus, SourceLanguage, UiSpec,
    };
    use crate::models::PathResolver;
    use std::path::PathBuf;

    /// Distinctive phrase that occurs exactly once inside
    /// `TECHNICAL_SYSTEM_PROMPT` and nowhere in the per-language guides, so
    /// counting it measures how many technical-policy copies a payload carries.
    const POLICY_ANCHOR: &str = "If the image parameter is empty, the displacement should no-op";

    fn policy_copy_count(haystack: &str) -> usize {
        haystack.matches(POLICY_ANCHOR).count()
    }

    fn legacy_design_output(macro_code: &str) -> DesignOutput {
        DesignOutput {
            title: "Bracket".to_string(),
            version_name: "V1".to_string(),
            response: String::new(),
            interaction_mode: InteractionMode::Design,
            macro_dialect: MacroDialect::Legacy,
            engine_kind: EngineKind::Freecad,
            source_language: SourceLanguage::LegacyPython,
            geometry_backend: GeometryBackend::Freecad,
            macro_code: macro_code.to_string(),
            ui_spec: UiSpec::default(),
            initial_params: Default::default(),
            post_processing: None,
        }
    }

    fn prompt_context(macro_code: &str) -> PromptContext {
        PromptContext {
            thread_id: "thread-1".to_string(),
            thread_title: "Thread A".to_string(),
            summary: "Thread: Bracket\n\nRecent user intents:\n- add a fillet".to_string(),
            recent_dialogue: "USER: add a fillet\nECKY EINACS: done".to_string(),
            pinned_references: String::new(),
            available_assets: String::new(),
            last_output: Some(legacy_design_output(macro_code)),
            design_digest: "Current working snapshot\nBracket [V1] (legacyPython)".to_string(),
            artifact_digest: String::new(),
        }
    }

    fn freecad_target() -> ResolvedAuthoringContext {
        ResolvedAuthoringContext {
            engine_kind: EngineKind::Freecad,
            source_language: SourceLanguage::LegacyPython,
            geometry_backend: GeometryBackend::Freecad,
        }
    }

    /// Assemble the two API message bodies exactly the way the generation
    /// command does: a system message from `design_system_prompt` (static
    /// output contract + language + applicable framework, once) and a user
    /// message routed through the typed generation envelope (current state +
    /// current ask once, no duplicate `EXECUTION RULES`). On mandatory overflow
    /// the envelope refuses to dispatch and the budget error text (observed /
    /// allowed sizes) stands in for the user body, which stays bounded.
    fn assemble_api_generation_payload(ctx: &PromptContext, base_prompt: &str) -> (String, String) {
        let target = freecad_target();
        let system = design_system_prompt(target.source_language, target.geometry_backend, None);
        let user = match assemble_generation_payload(ctx, base_prompt, "DESIGN_EDIT", target) {
            Ok(payload) => payload.user_content,
            Err(budget_err) => budget_err.to_string(),
        };
        (system, user)
    }

    // ── 1.1 provider/API payload snapshot (one copy / stable prefix / variable tail) ──
    #[test]
    fn api_payload_has_one_policy_copy_with_stable_prefix_and_variable_tail() {
        let ctx_a = prompt_context(
            "import FreeCAD\nbox = App.ActiveDocument.addObject('Part::Box','Box')\n",
        );
        let ctx_b = prompt_context(
            "import FreeCAD\nbox = App.ActiveDocument.addObject('Part::Box','Box')\n",
        );

        let (system_a, user_a) =
            assemble_api_generation_payload(&ctx_a, "increase the fillet radius");
        let (system_b, user_b) =
            assemble_api_generation_payload(&ctx_b, "add a counterbore on the top face");

        // (1) GREEN evidence — stable system prefix: same provider/backend/
        //     language/output contract yields a byte-identical system message so
        //     exact-prefix caching can work. Already holds today.
        assert_eq!(
            system_a, system_b,
            "system prefix must be byte-identical across turns sharing backend/language"
        );

        // (2) GREEN evidence — variable user tail: thread-specific request/state
        //     follows the stable prefix and differs between turns.
        assert_ne!(
            user_a, user_b,
            "user tail must vary with the current request"
        );

        // (3) GREEN — one technical-policy copy: the output rules occur
        //     exactly once across (system + user). The system message carries
        //     them once in the stable prefix; the user message no longer
        //     re-embeds them as `EXECUTION RULES`.
        let combined_policy_copies = policy_copy_count(&system_a) + policy_copy_count(&user_a);
        assert_eq!(
            combined_policy_copies, 1,
            "technical policy must appear exactly once in the assembled payload; \
             found {combined_policy_copies} copies (duplicate EXECUTION RULES in user content)"
        );
    }

    // ── 1.1 total-size metadata accompanying the assembled payload ──────────
    #[test]
    fn api_generation_payload_carries_total_size_metadata() {
        // Kept separate from the duplicate-policy assertion so the size-metadata
        // requirement is independently confirmable, not hidden behind the
        // duplicate-prompt failure.
        let ctx = prompt_context(
            "import FreeCAD\nbox = App.ActiveDocument.addObject('Part::Box','Box')\n",
        );
        let (_system, user) = assemble_api_generation_payload(&ctx, "increase the fillet radius");

        // GREEN — the assembled generation payload is accompanied by typed
        // total-size metadata (total chars / ceiling) covering the FULL user
        // content (sections plus the structural wrapper), produced by routing
        // the whole variable context through the typed generation envelope.
        let payload = assemble_generation_payload(
            &ctx,
            "increase the fillet radius",
            "DESIGN_EDIT",
            freecad_target(),
        )
        .expect("normal-sized context assembles without overflow");
        assert!(
            payload.envelope.total_returned_chars >= user.chars().count(),
            "generation payload must carry total-size metadata covering the full user content \
             ({} chars); the routed envelope accounts for {} chars",
            user.chars().count(),
            payload.envelope.total_returned_chars
        );
    }

    // ── 1.2 mandatory-overflow pre-dispatch failure ─────────────────────────
    #[test]
    fn mandatory_overflow_fails_pre_dispatch_instead_of_unbounded_assembly() {
        // Authoritative current source larger than the 64K generation ceiling.
        // Mandatory authoritative state must never be silently truncated, so the
        // generation assembly must refuse to dispatch and surface a raw
        // context-budget error naming observed/allowed section sizes — not embed
        // the oversized source verbatim and dispatch a lossy request.
        let overflow_source = "x".repeat(GENERATION_CEILING_CHARS + 6_000);
        let ctx = prompt_context(&overflow_source);

        let (_system, user) = assemble_api_generation_payload(&ctx, "edit the oversized model");

        // GREEN — the generation assembly refuses to dispatch oversized
        // authoritative state. Mandatory overflow fails pre-dispatch with a raw
        // context-budget error (observed/allowed section sizes), observable here
        // as the assembled user content never exceeding the ceiling (the budget
        // error text stands in for the user body on overflow).
        assert!(
            user.chars().count() <= GENERATION_CEILING_CHARS,
            "mandatory overflow must fail pre-dispatch with a raw context-budget error \
             (observed/allowed section sizes); instead the generation assembly produced a \
             {}-char user payload past the {}-char ceiling",
            user.chars().count(),
            GENERATION_CEILING_CHARS
        );
    }

    struct BindingTestResolver {
        root: PathBuf,
    }

    impl PathResolver for BindingTestResolver {
        fn app_config_dir(&self) -> PathBuf {
            self.root.clone()
        }

        fn app_data_dir(&self) -> PathBuf {
            self.root.clone()
        }

        fn resource_path(&self, _path: &str) -> Option<PathBuf> {
            None
        }
    }

    #[test]
    fn blank_ui_generation_creates_default_bound_source_immediately() {
        let root = std::env::temp_dir().join(format!(
            "ecky-ui-generation-binding-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let conn = crate::db::init_db(&root.join("history.sqlite")).expect("db");
        let resolver = BindingTestResolver { root };

        ensure_generation_thread_binding(
            &conn,
            &resolver,
            None,
            "thread-ui-1",
            "Make a bracket",
            100,
        )
        .expect("bind blank UI thread");

        let binding = crate::thread_source_binding::get_binding(&conn, "thread-ui-1")
            .unwrap()
            .expect("binding row");
        assert_eq!(
            std::fs::read_to_string(&binding.source_path).unwrap(),
            crate::thread_source_binding::DEFAULT_THREAD_SOURCE
        );
        assert!(PathBuf::from(binding.folder_path)
            .join(crate::project_mirror::PROJECT_MANIFEST_FILE_NAME)
            .is_file());
    }

    #[test]
    fn generated_source_is_persisted_before_checks_and_repairs_append() {
        let root =
            std::env::temp_dir().join(format!("ecky-generation-version-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let conn = crate::db::init_db(&root.join("history.sqlite")).expect("db");
        crate::db::create_or_update_thread(&conn, "thread-1", "Bracket", 1, None).unwrap();
        let pending = Message {
            id: "pending-1".into(),
            role: MessageRole::Assistant,
            content: "Generating...".into(),
            status: MessageStatus::Pending,
            output: None,
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            structural_verification: None,
            agent_origin: None,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
            timestamp: 2,
        };
        crate::db::add_message_checked(&conn, "thread-1", &pending).unwrap();

        let first = legacy_design_output("(model (part body (box 1 1 1)))");
        let first_id =
            persist_generation_draft_in_db(&conn, pending.id.clone(), first.clone(), None).unwrap();
        assert_eq!(first_id, pending.id);
        assert_eq!(
            crate::db::get_thread_head_version_id(&conn, "thread-1")
                .unwrap()
                .as_deref(),
            Some("pending-1")
        );

        let second = legacy_design_output("(model (part body (box 2 1 1)))");
        let second_id =
            persist_generation_draft_in_db(&conn, first_id, second.clone(), None).unwrap();
        assert_ne!(second_id, "pending-1");
        let identical_id =
            persist_generation_draft_in_db(&conn, second_id.clone(), second, None).unwrap();
        assert_eq!(identical_id, second_id);
        let version_count = crate::db::get_thread_messages_for_thread_view(&conn, "thread-1")
            .unwrap()
            .into_iter()
            .filter(|message| message.output.is_some())
            .count();
        assert_eq!(version_count, 2);
    }

    #[test]
    fn pending_placeholder_does_not_become_duplicate_version_for_identical_source() {
        let root = std::env::temp_dir().join(format!(
            "ecky-generation-identical-pending-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let conn = crate::db::init_db(&root.join("history.sqlite")).expect("db");
        crate::db::create_or_update_thread(&conn, "thread-1", "Bracket", 1, None).unwrap();

        let existing = legacy_design_output("(model (part body (box 1 1 1)))");
        let existing_message = Message {
            id: "version-a".into(),
            role: MessageRole::Assistant,
            content: existing.response.clone(),
            status: MessageStatus::Success,
            output: Some(existing.clone()),
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            structural_verification: None,
            agent_origin: None,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
            timestamp: 2,
        };
        crate::db::add_legacy_message(&conn, "thread-1", &existing_message).unwrap();

        let pending = Message {
            id: "pending-1".into(),
            role: MessageRole::Assistant,
            content: "Generating...".into(),
            status: MessageStatus::Pending,
            output: None,
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            structural_verification: None,
            agent_origin: None,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
            timestamp: 3,
        };
        crate::db::add_message_checked(&conn, "thread-1", &pending).unwrap();

        let version_id =
            persist_generation_draft_in_db(&conn, pending.id.clone(), existing.clone(), None)
                .unwrap();

        assert_eq!(version_id, "version-a");
        assert_eq!(
            crate::db::get_thread_head_version_id(&conn, "thread-1")
                .unwrap()
                .as_deref(),
            Some("version-a")
        );
        let version_count = crate::db::get_thread_messages_for_thread_view(&conn, "thread-1")
            .unwrap()
            .into_iter()
            .filter(|message| message.output.is_some())
            .count();
        assert_eq!(version_count, 1);
        let (pending_status, pending_output): (MessageStatus, Option<String>) = conn
            .query_row(
                "SELECT status, output FROM messages WHERE id = 'pending-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(pending_status, MessageStatus::Discarded);
        assert!(pending_output.is_none());
    }
}
