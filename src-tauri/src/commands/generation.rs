use base64::{engine::general_purpose, Engine as _};
use std::collections::BTreeMap;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, State};
use uuid::Uuid;

use super::session::{build_runtime_snapshot, write_last_snapshot};
use crate::context::*;
use crate::models::{
    validate_design_output, AppError, AppErrorCode, AppResult, AppState, ArtifactBundle,
    Attachment, AttachmentKind, DesignOutput, FinalizeStatus, GenerateDesignOptions,
    GenerateOutput, IntentDecision, InteractionMode, MacroDialect, Message, MessageRole,
    MessageStatus, ModelManifest, StructuralVerificationResult, UiSpec, UsageSummary,
};
use crate::services::design::{auto_heal_legacy_params, is_param_schema_mismatch};
use crate::{
    db, fallback_intent, freecad, llm, persist_thread_summary, persist_user_prompt_references,
    TECHNICAL_SYSTEM_PROMPT,
};

/// Per-language documentation appended to the API-mode system prompt.
///
/// FreeCAD Python and build123d are publicly documented, so a short recall note plus
/// the app-specific runtime contract is enough. Ecky is proprietary and unknown to
/// models, so a compact authoring guide is embedded.
pub fn language_guide_text(
    source_language: crate::models::SourceLanguage,
    geometry_backend: crate::models::GeometryBackend,
) -> String {
    match source_language {
        crate::models::SourceLanguage::EckyIrV0 => format!(
            "Ecky is a proprietary in-app CAD language. It is NOT publicly documented; \
             this guide and the example below are the complete authoritative reference. \
             Do not invent forms, ops, or keywords beyond what is listed here.\n\n{}\n\
             Prefer the smallest model that satisfies the request, then add named structure.\n",
            ecky_ir_v0_guide_text(geometry_backend)
        ),
        crate::models::SourceLanguage::Build123d => build123d_python_guide_text(),
        crate::models::SourceLanguage::LegacyPython => freecad_python_guide_text(),
    }
}

/// Full system prompt for API-mode design generation: base technical contract plus
/// documentation for the target source language.
pub fn design_system_prompt(
    source_language: crate::models::SourceLanguage,
    geometry_backend: crate::models::GeometryBackend,
) -> String {
    format!(
        "{}\n\nTARGET LANGUAGE GUIDE (AUTHORITATIVE FOR `macro_code`)\n{}",
        TECHNICAL_SYSTEM_PROMPT,
        language_guide_text(source_language, geometry_backend)
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

pub fn ecky_ir_v0_guide_text(backend: crate::models::GeometryBackend) -> String {
    match backend {
        crate::models::GeometryBackend::Build123d => ecky_build123d_guide_text(),
        crate::models::GeometryBackend::Freecad => freecad_guide_text(),
        _ => ecky_source_guide_text(),
    }
}

pub fn build123d_guide_text() -> String {
    ecky_build123d_guide_text()
}

pub fn build123d_python_guide_text() -> String {
    concat!(
        "Return canonical `build123d` source in `macro_code`.\n",
        "Current fileExtension: `.py`.\n",
        "Current sourceLanguage: `build123d`.\n",
        "build123d is a publicly documented Python CAD library; use standard, well-known API only.\n",
        "Start with `from build123d import *`.\n\n",
        "Rules:\n",
        "- Use `with BuildPart() as body:` or similar containers to define parts.\n",
        "- To expose dynamic parameters, use the `params` dictionary which will be provided at runtime.\n",
        "- Example: `radius = params.get('radius', 10.0)`\n",
        "- Ensure the final shapes are added to the `_ecky_parts` list if you want them exported as separate objects.\n",
        "- Example: `_ecky_parts = [('body', body.part)]` (list of tuples: label, shape)\n\n",
        "Example:\n",
        "from build123d import *\n",
        "with BuildPart() as lamp:\n",
        "    Cylinder(radius=params.get('radius', 20), height=params.get('height', 100))\n",
        "_ecky_parts = [('lamp', lamp.part)]\n"
    ).to_string()
}

fn ecky_backend_guide_text(
    backend: crate::models::GeometryBackend,
    backend_label: &str,
    implicit_backend: bool,
) -> String {
    let surface = crate::ecky_language_surface::supported_surface_manifest(backend);
    let target_line = if implicit_backend {
        "Target geometryBackend comes from current request, model version, or config default; never from thread metadata.\n".to_string()
    } else {
        format!("Target geometryBackend: `{backend_label}`.\n")
    };
    let backend_note = if matches!(backend, crate::models::GeometryBackend::EckyRust) {
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
    let expression_forms = crate::ecky_language_surface::join_backticked(surface.expression_forms);
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
- Top-level model clauses: {model_clauses}. Use `params`, `part`, and `meta` directly under `model`.\n\
- Supported expression forms: {expression_forms}. Use `let*` when later bindings depend on earlier ones.\n\
- Use `map`, `range`, `repeat-union`, and `repeat-compound` inside geometry, not to generate top-level clauses.\n\
- Static tuple destructuring is supported only for `zip` and `enumerate` static sources: `(map (lambda ((x y)) ...) (zip xs ys))`. Zip destructuring of a dynamic source rejects with a clear error.\n\
- Supported CAD ops for this backend: {supported_ops}.\n\
- Numeric helpers: {numeric_helpers}. Point/list helpers: {point_helpers}. Bounded literal counts/steps only. Seeded helpers are deterministic for a given seed.\n\
- Keywords are not callable nodes: write `(box 10 10 2 :align '(center center min))`, never `(align ...)`.\n\
- Name fit-critical bindings before use: `wall`, `clearance`, `bore-r`, `top-z`. No anonymous offsets for fit-critical geometry.\n\
- For generated Ecky models, write top-level `(verify ...)` clauses in the same `(model ...)` from the user's measurable requirements before trusting geometry; a red first render is expected repair input.\n\
- Verify with typed/static errors and structural verification first, screenshots last.\n\
{backend_note}{wall_patterns}\n\
PARAMS\n\
- `(number key default :label \"...\" :min n :max n :step n)`\n\
- `(select key \"default\" :label \"...\" :options ((\"Label\" \"value\") ...))`\n\
- `(toggle key #t :label \"...\")`\n\
- `(image key \"\" :label \"...\")`\n\n\
VERIFY CLAUSES\n\
- Purpose: make source carry machine-checkable intent. The model writes `(verify ...)`; app verification evaluates it later.\n\
- Put `verify` directly under `model`, before or after `part` clauses. Never nest `verify` inside geometry, params, `build`, `let`, or components.\n\
- Clause grammar: `(verify (tag stable-name optional.selector ...) (metric alias (namespace key optional.args ...)) (expect alias (operator literal)))`.\n\
- Required section order: `tag`, then `metric`, then `expect`. Empty `(verify)` is invalid.\n\
- `tag` carries authored labels or selectors for diagnostics; use stable names like `mesh_clean`, `lid_gap`, `preview_exists`.\n\
- `metric` first item is a local alias; second item is a metric expression. `expect` alias must match the metric alias exactly.\n\
- Metric namespaces: `manifest`, `stl`, `clearance`, `selector`, `relation`.\n\
- Manifest metrics: `(manifest has-step)`, `(manifest has-preview-stl)`, `(manifest edge-target-count)`, `(manifest face-target-count)`, `(manifest export-format-count)`, `(manifest part-count)`.\n\
- STL metrics: `(stl triangle-count)`, `(stl connected-component-count)`, `(stl non-manifold-edge-count)`, `(stl overhang-face-count)`.\n\
- Clearance metric: `(clearance min-distance selector-a selector-b)`. Selectors may be part names such as `body` and `lid`, or stable target ids when known.\n\
- Selector metrics: `(selector axis selector)`, `(selector extent-x selector)`, `(selector extent-y selector)`, `(selector extent-z selector)`, `(selector center-x selector)`, `(selector center-y selector)`, `(selector center-z selector)`. Axis returns text: `x`, `y`, or `z`; extents and centers are millimeters.\n\
- Relation metrics: `(relation axis-angle selector-a selector-b)`, `(relation center-delta-x selector-a selector-b)`, `(relation center-delta-y selector-a selector-b)`, `(relation center-delta-z selector-a selector-b)`. Axis angle is unsigned degrees; center deltas are signed millimeters: selector-a center minus selector-b center.\n\
- Operators: `=`, `!=`, `>`, `>=`, `<`, `<=`. Literals may be boolean, number, or text; do not use params or computed expressions in `expect` literals.\n\
- Good default checks for generated `.ecky`: preview STL exists, part count is positive, STL triangle count is positive, non-manifold edge count is zero.\n\
- Use clearance verification when the request names a fit/gap/clearance. Use numeric literals matching the promised clearance.\n\
- Use selector/relation verification when the request names orientation, fit axis, length, width, thickness, center offset, or perpendicular/parallel relation.\n\
- Do not remove or weaken existing `(verify ...)` clauses during repair; change geometry or params until they pass.\n\
```ecky\n\
(model\n\
  (verify\n\
    (tag preview_exists)\n\
    (metric check (manifest has-preview-stl))\n\
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

fn ecky_build123d_guide_text() -> String {
    ecky_backend_guide_text(
        crate::models::GeometryBackend::Build123d,
        "build123d",
        false,
    )
}

pub fn freecad_guide_text() -> String {
    ecky_backend_guide_text(crate::models::GeometryBackend::Freecad, "freecad", false)
}

pub fn ecky_source_guide_text() -> String {
    ecky_backend_guide_text(crate::models::GeometryBackend::EckyRust, "mesh", true)
}

fn selected_engine(state: &State<'_, AppState>) -> AppResult<crate::models::Engine> {
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

fn default_engine_kind(app_state: &AppState) -> crate::models::EngineKind {
    app_state.config.lock().unwrap().default_engine_kind
}

fn default_source_language(app_state: &AppState) -> crate::models::SourceLanguage {
    app_state.config.lock().unwrap().default_source_language
}

fn default_geometry_backend(app_state: &AppState) -> crate::models::GeometryBackend {
    app_state.config.lock().unwrap().default_geometry_backend
}

async fn resolve_generation_engine_kind(
    app_state: &AppState,
    _thread_id: Option<&str>,
    explicit: Option<crate::models::EngineKind>,
    working_design: Option<&DesignOutput>,
    last_output: Option<&DesignOutput>,
) -> AppResult<crate::models::EngineKind> {
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
    explicit: Option<crate::models::SourceLanguage>,
    working_design: Option<&DesignOutput>,
    last_output: Option<&DesignOutput>,
) -> AppResult<crate::models::SourceLanguage> {
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
    explicit: Option<crate::models::GeometryBackend>,
    working_design: Option<&DesignOutput>,
    last_output: Option<&DesignOutput>,
) -> AppResult<crate::models::GeometryBackend> {
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

fn build_available_assets_block(state: &State<'_, AppState>, app: &AppHandle) -> String {
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

fn load_framework_contract(app: &AppHandle) -> Option<String> {
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

fn should_use_framework_for_generation(ctx: &PromptContext) -> bool {
    let _ = ctx;
    true
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
    {
        let (explicit_mcp, no_enabled_engine) = {
            let config = state.config.lock().unwrap();
            let explicit_mcp = config.connection_type.as_deref() == Some("mcp");
            let no_enabled_engine = !config.engines.iter().any(|e| e.enabled);
            (explicit_mcp, no_enabled_engine)
        };

        if explicit_mcp || no_enabled_engine {
            let sessions = state.mcp_sessions.lock().await;
            if sessions.is_empty() && no_enabled_engine {
                return Err(AppError::validation(
                    "No active engine or MCP agent found. Switch to API Key mode in Settings → Agents, or connect an agent.",
                ));
            }
            // In MCP mode the frontend routes user input through request_user_prompt,
            // not through generate. If we somehow get here, do nothing.
            return Err(AppError::validation(
                "In MCP mode, generation is driven by your external agent.",
            ));
        }
    }
    let engine = selected_engine(&state)?;
    let options = options.unwrap_or_default();
    let mut ctx = {
        let db = state.db.lock().await;
        crate::context::assemble_context(
            &db,
            thread_id.clone(),
            working_design.clone(),
            parent_macro_code.clone(),
        )
    };
    let engine_kind = resolve_generation_engine_kind(
        state.inner(),
        thread_id.as_deref(),
        options.engine_kind,
        working_design.as_ref(),
        ctx.last_output.as_ref(),
    )
    .await?;
    let source_language = resolve_generation_source_language(
        state.inner(),
        thread_id.as_deref(),
        options.source_language,
        working_design.as_ref(),
        ctx.last_output.as_ref(),
    )
    .await?;
    let geometry_backend = resolve_generation_geometry_backend(
        state.inner(),
        thread_id.as_deref(),
        options.geometry_backend,
        working_design.as_ref(),
        ctx.last_output.as_ref(),
    )
    .await?;
    let question_mode = options.question_mode.unwrap_or(false);
    let follow_up_question = options.follow_up_question;
    ctx.available_assets = build_available_assets_block(&state, &app);
    let intent_mode = if question_mode {
        "QUESTION_ONLY"
    } else {
        "DESIGN_EDIT"
    };
    let framework_enabled = should_use_framework_for_generation(&ctx);
    let framework_contract = if framework_enabled {
        load_framework_contract(&app)
    } else {
        None
    };
    let contextual_prompt = format_contextual_prompt(
        &ctx,
        &prompt,
        TECHNICAL_SYSTEM_PROMPT,
        intent_mode,
        framework_contract.as_deref(),
        crate::context::ResolvedAuthoringContext {
            engine_kind,
            source_language,
            geometry_backend,
        },
    );
    let contextual_prompt =
        prepend_follow_up_context(contextual_prompt, follow_up_question.as_deref());
    let contextual_prompt =
        if let Some(notes) = build_visual_input_notes(image_data.as_ref(), attachments.as_ref()) {
            format!("{}\n\n{}", contextual_prompt, notes)
        } else {
            contextual_prompt
        };
    let images = prepare_images(image_data, attachments);

    let system_prompt = design_system_prompt(source_language, geometry_backend);
    let mut output = llm::generate_design(&engine, &system_prompt, &contextual_prompt, images)
        .await
        .map_err(|raw_body| {
            AppError::with_details(
                AppErrorCode::Provider,
                "LLM response could not be parsed into a design output.",
                raw_body,
            )
        })?;

    if !question_mode {
        if framework_enabled {
            if engine_kind == crate::models::EngineKind::EckyIrV0 {
                output.data.macro_dialect = MacroDialect::EckyIrV0;
            } else if let Some(parsed) =
                crate::commands::design::derive_framework_controls(&output.data.macro_code)?
            {
                output.data.ui_spec = UiSpec {
                    fields: parsed.fields.clone(),
                };
                output.data.initial_params = parsed.params;
                output.data.macro_dialect = MacroDialect::CadFrameworkV1;
            } else {
                output.data.macro_dialect = MacroDialect::Legacy;
            }
        } else {
            output.data.macro_dialect =
                if source_language == crate::models::SourceLanguage::EckyIrV0 {
                    MacroDialect::EckyIrV0
                } else if source_language == crate::models::SourceLanguage::Build123d {
                    MacroDialect::Build123d
                } else {
                    MacroDialect::Legacy
                };
        }
        output.data.engine_kind = engine_kind;
        output.data.source_language = source_language;
        output.data.geometry_backend = geometry_backend;
    }

    if question_mode {
        output.data.interaction_mode = InteractionMode::Question;
        if let Some(previous) = &ctx.last_output {
            output.data.title = previous.title.clone();
            output.data.version_name = previous.version_name.clone();
            output.data.macro_code = previous.macro_code.clone();
            output.data.ui_spec = previous.ui_spec.clone();
            output.data.initial_params = previous.initial_params.clone();
            output.data.macro_dialect = previous.macro_dialect.clone();
            output.data.engine_kind = previous.engine_kind;
            output.data.source_language = previous.source_language;
            output.data.geometry_backend = previous.geometry_backend;
        }
        if output.data.version_name.trim().is_empty() {
            output.data.version_name = "Q&A".to_string();
        }
        if output.data.response.trim().is_empty() {
            output.data.response = "Question answered. Geometry unchanged.".to_string();
        }
    }

    if let Err(err) = validate_design_output(&output.data) {
        if !question_mode
            && output.data.macro_dialect == MacroDialect::Legacy
            && is_param_schema_mismatch(&err)
        {
            if let Some((ui_spec, initial_params, _heal_report)) = auto_heal_legacy_params(
                &output.data.macro_code,
                &output.data.ui_spec,
                &output.data.initial_params,
                ctx.last_output
                    .as_ref()
                    .map(|design| &design.initial_params),
            )? {
                output.data.ui_spec = ui_spec;
                output.data.initial_params = initial_params;
                validate_design_output(&output.data)?;
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
        design: output.data,
        thread_id: ctx.thread_id,
        message_id: Uuid::new_v4().to_string(),
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
    state: State<'_, AppState>,
) -> AppResult<String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let assistant_message_id = Uuid::new_v4().to_string();
    let user_message_id = Uuid::new_v4().to_string();

    {
        let db = state.db.lock().await;
        if db::get_thread_title(&db, &thread_id)
            .map_err(|err| AppError::persistence(err.to_string()))?
            .is_none()
        {
            let traits = crate::generate_genie_traits();
            let initial_title = {
                let chars: Vec<char> = prompt.chars().collect();
                if chars.len() > 30 {
                    format!("{}...", chars[..27].iter().collect::<String>())
                } else {
                    prompt.clone()
                }
            };
            db::create_or_update_thread(&db, &thread_id, &initial_title, now, Some(&traits))
                .map_err(|err| AppError::persistence(err.to_string()))?;
        }

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
        db::add_message(&db, &thread_id, &user_msg)
            .map_err(|err| AppError::persistence(err.to_string()))?;
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
        db::add_message(&db, &thread_id, &assistant_msg)
            .map_err(|err| AppError::persistence(err.to_string()))?;
    }

    Ok(assistant_message_id)
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
    if let Some(design) = design.as_ref() {
        validate_design_output(design)?;
    }

    let db = state.db.lock().await;
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
        let thread_id = if let Some((_, _, thread_id)) =
            db::get_message_runtime_and_thread(&db, &message_id)
                .map_err(|err| AppError::persistence(err.to_string()))?
        {
            Some(thread_id)
        } else {
            db::get_message_output_and_thread(&db, &message_id)
                .map_err(|err| AppError::persistence(err.to_string()))?
                .map(|(_, thread_id)| thread_id)
        };

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

            if design.is_some() || artifact_bundle.is_some() || model_manifest.is_some() {
                let snapshot = build_runtime_snapshot(
                    design,
                    Some(thread_id.clone()),
                    Some(message_id.clone()),
                    artifact_bundle,
                    model_manifest,
                    None,
                );
                {
                    let mut last = state.last_snapshot.lock().unwrap();
                    *last = Some(snapshot.clone());
                }
                write_last_snapshot(&app, Some(&snapshot));
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
    db::update_message_structural_verification(&db, &message_id, Some(&structural_verification))
        .map_err(|err| AppError::persistence(err.to_string()))?;
    Ok(())
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
    let engine = selected_engine(&state)?;
    let explicit_question_only = crate::is_explicit_question_only_request(&prompt);
    let backend_context = if thread_id.is_some() {
        let ctx = {
            let db = state.db.lock().await;
            crate::context::assemble_context(&db, thread_id, None, None)
        };
        let mut blocks = Vec::new();
        if !ctx.summary.trim().is_empty() {
            blocks.push(format!("THREAD SUMMARY\n{}", ctx.summary));
        }
        if !ctx.recent_dialogue.trim().is_empty() {
            blocks.push(format!("RECENT DIALOGUE\n{}", ctx.recent_dialogue));
        }
        if !ctx.pinned_references.trim().is_empty() {
            blocks.push(format!("PINNED REFERENCES\n{}", ctx.pinned_references));
        }
        if !ctx.design_digest.trim().is_empty() {
            blocks.push(format!(
                "ACTUAL LIVE DESIGN DIGEST (AUTHORITATIVE)\n{}",
                ctx.design_digest
            ));
        }
        if let Some(frontend_context) = context.as_ref().filter(|value| !value.trim().is_empty()) {
            blocks.push(format!(
                "ACTUAL LIVE WORKING SNAPSHOT (FRONTEND)\n{}",
                frontend_context
            ));
        }
        Some(blocks.join("\n\n"))
    } else {
        context
    };

    let prompt =
        if let Some(notes) = build_visual_input_notes(image_data.as_ref(), attachments.as_ref()) {
            format!("{}\n\n{}", prompt, notes)
        } else {
            prompt
        };
    let images = prepare_images(image_data, attachments);
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
            Ok(IntentDecision {
                intent_mode: if explicit_question_only {
                    "question".to_string()
                } else {
                    intent
                },
                confidence,
                response,
                final_response,
                usage: classification.usage,
            })
        }
        Err(_) => Ok(fallback_intent(&prompt)),
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
        build123d_guide_text, build123d_python_guide_text, ecky_ir_v0_guide_text,
        freecad_guide_text, prepend_follow_up_context, resolve_generation_engine_kind,
        resolve_generation_geometry_backend, resolve_generation_source_language,
        should_use_framework_for_generation,
    };
    use crate::context::PromptContext;
    use crate::contracts::{Config, McpConfig};
    use crate::models::{
        AppState, DesignOutput, EngineKind, GeometryBackend, InteractionMode, MacroDialect,
        SourceLanguage, UiSpec,
    };
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
            voice: crate::models::VoiceConfig::default(),
            mcp: McpConfig::default(),
            has_seen_onboarding: true,
            connection_type: None,
            default_engine_kind: EngineKind::Freecad,
            default_source_language: SourceLanguage::LegacyPython,
            default_geometry_backend: GeometryBackend::Freecad,
            max_generation_attempts: 3,
            max_verify_attempts: 0,
            projects_root: None,
        }
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
                    crate::models::EngineKind::EckyIrV0
                } else {
                    crate::models::EngineKind::Freecad
                },
                geometry_backend: if macro_dialect == MacroDialect::EckyIrV0 {
                    crate::models::GeometryBackend::EckyRust
                } else {
                    crate::models::GeometryBackend::Freecad
                },
                source_language: if macro_dialect == MacroDialect::EckyIrV0 {
                    crate::models::SourceLanguage::EckyIrV0
                } else {
                    crate::models::SourceLanguage::LegacyPython
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
        assert!(build123d.contains("(manifest has-preview-stl)"));
        assert!(build123d.contains("(manifest part-count)"));
        assert!(build123d.contains("(stl non-manifold-edge-count)"));
        assert!(build123d.contains("(stl triangle-count)"));
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
        assert!(build123d.contains("(metric check (manifest has-preview-stl))"));
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

        let ecky = ecky_ir_v0_guide_text(crate::models::GeometryBackend::EckyRust);
        assert!(ecky.contains("Current fileExtension: `.ecky`."));
        assert!(ecky.contains("Current sourceLanguage: `ecky`."));
        assert!(ecky.contains("never from thread metadata"));
        assert!(ecky.contains("renders through EckyRust CAD VM"));
        assert!(ecky.contains("Do not promise STEP"));
        assert!(ecky.contains("ArtifactBundle.exportArtifacts"));
        assert!(ecky.contains("PROGRESSIVE ECKY EXAMPLES"));
        assert!(ecky.contains("write top-level `(verify ...)` clauses"));
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
        let build123d = build123d_guide_text();
        let freecad = freecad_guide_text();
        let mesh = ecky_ir_v0_guide_text(crate::models::GeometryBackend::EckyRust);

        for op in crate::ecky_language_surface::CAD_OPS_PORTABLE {
            assert!(
                build123d.contains(&format!("`{op}`")),
                "build123d missing {op}"
            );
            assert!(freecad.contains(&format!("`{op}`")), "freecad missing {op}");
            assert!(mesh.contains(&format!("`{op}`")), "mesh missing {op}");
            assert!(
                crate::ecky_scheme::cad::MODULE.exports.contains(op),
                "manifest op not exported: {op}"
            );
        }
        assert!(!build123d.contains("`align`"));
        assert!(!freecad.contains("`align`"));
        assert!(!build123d.contains("`wall-pattern`"));
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
