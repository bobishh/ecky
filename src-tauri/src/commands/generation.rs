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
    MessageStatus, ModelManifest, UiSpec, UsageSummary,
};
use crate::services::design::{auto_heal_legacy_params, is_param_schema_mismatch};
use crate::{
    db, fallback_intent, freecad, llm, persist_thread_summary, persist_user_prompt_references,
    TECHNICAL_SYSTEM_PROMPT,
};

pub fn ecky_ir_v0_guide_text(backend: crate::models::GeometryBackend) -> String {
    match backend {
        crate::models::GeometryBackend::Build123d => ecky_ir_v0_guide_build123d(),
        _ => ecky_ir_v0_guide_ecky_rust(),
    }
}

fn ecky_ir_v0_guide_build123d() -> String {
    concat!(
        "Return canonical Ecky IR v0 source as compact ASCII s-expressions in `macro_code`.\n",
        "Start with `(model ...)`. Do not return Python.\n",
        "Active geometry backend: BUILD123D (OCCT). Produces solid, manifold geometry.\n\n",
        "Supported v0 ops:\n",
        "- Primitives: `box`, `cylinder`, `cone`, `sphere`, `rounded-rect`, `circle`, `polygon`.\n",
        "  NOTE: `rounded-polygon` and `bspline` are NOT supported on this backend — use `polygon` with enough points or `profile` instead.\n",
        "- Path nodes: `path` (raw 3D point list), `bezier-path` (cubic bezier chain from 3n+1 points).\n",
        "- Sketch nodes: `profile` with explicit `(:outer ...)` and `(:holes ...)` loops.\n",
        "- Sketch modifiers: `offset`, `offset-rounded`.\n",
        "- Constructive nodes: `extrude`, `revolve`, `loft`, `taper`, `twist`, `sweep`, `shell`.\n",
        "  NOTE: `wall-pattern` is NOT supported on this backend — use the EckyRust backend for patterned surfaces.\n",
        "- `shell` supported targets: `extrude`, `revolve`, `sweep`, `loft`, `cylinder`, `cone`, `sphere`.\n",
        "  `shell` does NOT support `taper` or `twist` targets on this backend.\n",
        "- Edge ops: `fillet`, `chamfer` — fully supported via OCCT.\n",
        "- Composition: `union`, `difference`, `intersection`, `xor`.\n",
        "- Transforms: `translate`, `rotate`, `scale`, `mirror`.\n",
        "- Arrays: `linear-array`, `radial-array`, `grid-array`, `arc-array`.\n",
        "- Numeric helpers: `clamp`, `lerp`, `smoothstep`, `+`, `-`, `*`, `/`, `min`, `max`, `abs`, `sin`, `cos`, `tan`, `deg`, `rad`.\n",
        "- Boolean ops: `not`, `and`, `or`, `=` (numeric or string equality), `>`, `>=`, `<`, `<=`.\n",
        "- Conditional: `(if bool-expr then-shape else-shape)`.\n\n",
        "Param types:\n",
        "- `(number key default :label \"...\" :min n :max n)` — numeric input.\n",
        "- `(select key \"default\" :label \"...\" :options ((\"Label\" \"val\") ...))` — dropdown.\n",
        "- `(toggle key #t :label \"...\")` — boolean checkbox; use `#t`/`#f`.\n",
        "- `(image key \"\" :label \"...\")` — file picker; leave default empty.\n\n",
        "Rules & Syntax:\n",
        "- Use `(profile (:outer ((x y) ...)) (:holes (((x y) ...))))` for contour-aware sketches with holes.\n",
        "- `extrude`, `revolve`, `loft`, `taper`, `twist`, `sweep` are all hole-aware and preserve internal cutouts.\n",
        "- `loft` requires compatible topology (same number of loops, same vertex count per mapped loop after resampling).\n",
        "- `revolve` and `sweep` produce solid OCCT geometry — reliable for curved shapes including domes.\n",
        "  For a dome: `(difference (sphere r 32) (translate 0 0 (- r) (box (* 2 r) (* 2 r) r)))` or\n",
        "  `(revolve (profile (:outer ((0 0) (r 0) ... ))) 360)`.\n",
        "- `fillet` and `chamfer`: `(fillet radius body)` for all edges, or `(fillet radius :edges top body)` for selective edges.\n",
        "  Edge selectors: `all` (default), `top`, `bottom`, `vertical`.\n",
        "- Array signatures: `(linear-array count dx dy dz mesh)`, `(grid-array rows cols dx dy mesh)`,\n",
        "  `(radial-array count step-deg radius mesh)`, `(arc-array count radius start-deg end-deg mesh)`.\n",
        "- Do not emit a `lithophane` source node. Drive lithophane through `postProcessing.lithophaneAttachments` / the guided LITHO tab.\n",
        "- Keep `ui_spec`/`initial_params` aligned with the IR parameters.\n\n",
        "Examples:\n\n",
        "Teapot spout (sweep + bezier-path):\n",
        "(model\n",
        "  (part spout\n",
        "    (sweep (circle 8 16)\n",
        "      (bezier-path ((0 0 0) (5 10 20) (10 30 40) (8 50 50)) 24))))\n\n",
        "Parametric cylinder:\n",
        "(model\n",
        "  (params (number dia 40 :label \"Diameter\" :min 10 :max 120)\n",
        "          (number h 60 :label \"Height\" :min 10 :max 200))\n",
        "  (part body (cylinder dia h 48)))\n\n",
        "Hollow rounded box:\n",
        "(model\n",
        "  (params (number w 80 :label \"Width\" :min 20 :max 200)\n",
        "          (number wall 2.5 :label \"Wall\" :min 1 :max 8))\n",
        "  (part body (shell wall (extrude (rounded-rect w 60 4) 50))))\n\n",
        "Dome (hemisphere via sphere + cut):\n",
        "(model\n",
        "  (params (number r 30 :label \"Radius\" :min 10 :max 100))\n",
        "  (part dome\n",
        "    (difference (sphere r 32)\n",
        "                (translate 0 0 (- r) (box (* 2 r) (* 2 r) r)))))\n\n",
        "Fillet all top edges:\n",
        "(model\n",
        "  (params (number w 40 :label \"Width\" :min 10 :max 100)\n",
        "          (number h 30 :label \"Height\" :min 5 :max 80))\n",
        "  (part body (fillet 3 :edges top (box w w h))))\n\n",
        "Radial array of spokes (step-deg = 360/count):\n",
        "(model\n",
        "  (params (number n 6 :label \"Count\" :min 3 :max 12))\n",
        "  (part wheel\n",
        "    (union (cylinder 8 4 24)\n",
        "           (radial-array n (/ 360 n) 30 (cylinder 3 4 12)))))\n\n",
        "Conditional cap via toggle:\n",
        "(model\n",
        "  (params (toggle cap #t :label \"Cap top\"))\n",
        "  (part body\n",
        "    (if cap\n",
        "      (union (cylinder 20 40 32) (translate 0 0 40 (sphere 20 32)))\n",
        "      (cylinder 20 40 32))))\n\n",
        "Parametric ring using math:\n",
        "(model\n",
        "  (params (number outer 30 :label \"Outer radius\" :min 10 :max 80)\n",
        "          (number wall 3 :label \"Wall\" :min 1 :max 10))\n",
        "  (part ring\n",
        "    (difference (cylinder outer 8 32)\n",
        "                (cylinder (- outer wall) 10 32))))"
    ).to_string()
}

fn ecky_ir_v0_guide_ecky_rust() -> String {
    concat!(
        "Return canonical Ecky IR v0 source as compact ASCII s-expressions in `macro_code`.\n",
        "Start with `(model ...)`. Do not return Python.\n",
        "Active geometry backend: ECKY RUST (CSG mesh). Experimental; good for organic shapes and wall patterns.\n\n",
        "Supported v0 ops:\n",
        "- Primitives: `box`, `cylinder`, `cone`, `sphere`, `rounded-rect`, `circle`, `polygon`, `rounded-polygon`, `bspline`.\n",
        "- Path nodes: `path` (raw 3D point list), `bezier-path` (cubic bezier chain from 3n+1 points).\n",
        "- Sketch nodes: `profile` with explicit `(:outer ...)` and `(:holes ...)` loops.\n",
        "- Sketch modifiers: `offset`, `offset-rounded`.\n",
        "- Constructive nodes: `extrude`, `revolve`, `loft`, `taper`, `twist`, `sweep`, `shell`, `wall-pattern`.\n",
        "- `shell` supported targets: `extrude`, `revolve`, `loft`, `taper`, `twist`, `sweep`, `cylinder`, `cone`, `sphere`.\n",
        "  `fillet` and `chamfer` are supported with native mesh approximation; prefer rounded/bspline profiles when edge continuity matters most.\n",
        "- Composition: `union`, `difference`, `intersection`, `xor`.\n",
        "- Transforms: `translate`, `rotate`, `scale`, `mirror`.\n",
        "- Arrays: `linear-array`, `radial-array`, `grid-array`, `arc-array`.\n",
        "- Numeric helpers: `clamp`, `lerp`, `smoothstep`, `+`, `-`, `*`, `/`, `min`, `max`, `abs`, `sin`, `cos`, `tan`, `deg`, `rad`.\n",
        "- Boolean ops: `not`, `and`, `or`, `=` (numeric or string equality), `>`, `>=`, `<`, `<=`.\n",
        "- Conditional: `(if bool-expr then-shape else-shape)`.\n\n",
        "Param types:\n",
        "- `(number key default :label \"...\" :min n :max n)` — numeric input.\n",
        "- `(select key \"default\" :label \"...\" :options ((\"Label\" \"val\") ...))` — dropdown.\n",
        "- `(toggle key #t :label \"...\")` — boolean checkbox; use `#t`/`#f`.\n",
        "- `(image key \"\" :label \"...\")` — file picker; leave default empty.\n\n",
        "Rules & Syntax:\n",
        "- Use `(profile (:outer ((x y) ...)) (:holes (((x y) ...))))` for contour-aware sketches with holes.\n",
        "- Use `(rounded-polygon ((x y) ...) radius segments?)` for rounded closed profiles.\n",
        "- Use `(bspline ((x y) ...) closed? samples?)` for smooth closed loops. Prefer closed `#t` loops for printable profiles.\n",
        "- `extrude`, `revolve`, `loft`, `taper`, `twist`, `sweep` are all hole-aware and preserve internal cutouts.\n",
        "- `loft` requires compatible topology (same number of loops, same vertex count per mapped loop after resampling).\n",
        "- `revolve` and `sweep` use CSG mesh generation. Complex curved closed shapes (domes, U-profiles) may produce\n",
        "  mesh artifacts — prefer primitive-based construction for domes:\n",
        "  `(difference (sphere r 32) (translate 0 0 (- r) (box (* 2 r) (* 2 r) r)))`.\n",
        "- `wall-pattern` supports shell-surface targets and deforms the outer surface only.\n",
        "- `wall-pattern` modes: `ribs` (vertical), `rings` (horizontal), `spiral`, `diamond`, `hammered`.\n",
        "- `wall-pattern` options: `:mode` (required; literal symbol like `ribs` OR a `select` param ref), `:depth` (required, mm),\n",
        "  `:uFreq` (default 8), `:vFreq` (default 0), `:softness` (0–1, default 0.15), `:duty` (0–1, default 0.5),\n",
        "  `:phase`, `:bias`, `:twistDeg`, `:seed`, `:rimFade`. All numeric options accept param references.\n",
        "- Array signatures: `(linear-array count dx dy dz mesh)`, `(grid-array rows cols dx dy mesh)`,\n",
        "  `(radial-array count step-deg radius mesh)`, `(arc-array count radius start-deg end-deg mesh)`.\n",
        "- Do not emit a `lithophane` source node. Drive lithophane through `postProcessing.lithophaneAttachments` / the guided LITHO tab.\n",
        "- Keep `ui_spec`/`initial_params` aligned with the IR parameters.\n\n",
        "Examples:\n\n",
        "Teapot spout (sweep + bezier-path):\n",
        "(model\n",
        "  (part spout\n",
        "    (sweep (circle 8 16)\n",
        "      (bezier-path ((0 0 0) (5 10 20) (10 30 40) (8 50 50)) 24))))\n\n",
        "Parametric cylinder:\n",
        "(model\n",
        "  (params (number dia 40 :label \"Diameter\" :min 10 :max 120)\n",
        "          (number h 60 :label \"Height\" :min 10 :max 200))\n",
        "  (part body (cylinder dia h 48)))\n\n",
        "Hollow rounded box:\n",
        "(model\n",
        "  (params (number w 80 :label \"Width\" :min 20 :max 200)\n",
        "          (number wall 2.5 :label \"Wall\" :min 1 :max 8))\n",
        "  (part body (shell wall (extrude (rounded-rect w 60 4) 50))))\n\n",
        "Ribbed vase (wall-pattern):\n",
        "(model\n",
        "  (params (number r 18 :label \"Radius\" :min 8 :max 50)\n",
        "          (number ribs 20 :label \"Rib count\" :min 6 :max 40))\n",
        "  (part vase\n",
        "    (wall-pattern (:mode ribs :depth 1.2 :uFreq ribs :softness 0.12)\n",
        "      (shell 2 (extrude (circle r 48) 90)))))\n\n",
        "Extruded profile with hole (rounded-polygon):\n",
        "(model\n",
        "  (part frame\n",
        "    (extrude\n",
        "      (profile\n",
        "        (:outer (rounded-polygon ((0 40) (40 0) (0 -40) (-40 0)) 5 8))\n",
        "        (:holes (circle 20 24)))\n",
        "      6)))\n\n",
        "Radial array of spokes (step-deg = 360/count):\n",
        "(model\n",
        "  (params (number n 6 :label \"Count\" :min 3 :max 12))\n",
        "  (part wheel\n",
        "    (union (cylinder 8 4 24)\n",
        "           (radial-array n (/ 360 n) 30 (cylinder 3 4 12)))))\n\n",
        "Conditional cap via toggle:\n",
        "(model\n",
        "  (params (toggle cap #t :label \"Cap top\"))\n",
        "  (part body\n",
        "    (if cap\n",
        "      (union (cylinder 20 40 32) (translate 0 0 40 (sphere 20 32)))\n",
        "      (cylinder 20 40 32))))\n\n",
        "Parametric ring using math:\n",
        "(model\n",
        "  (params (number outer 30 :label \"Outer radius\" :min 10 :max 80)\n",
        "          (number wall 3 :label \"Wall\" :min 1 :max 10))\n",
        "  (part ring\n",
        "    (difference (cylinder outer 8 32)\n",
        "                (cylinder (- outer wall) 10 32))))"
    ).to_string()
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

fn default_engine_kind(state: &State<'_, AppState>) -> crate::models::EngineKind {
    state.config.lock().unwrap().default_engine_kind
}

fn default_source_language(state: &State<'_, AppState>) -> crate::models::SourceLanguage {
    state.config.lock().unwrap().default_source_language
}

fn default_geometry_backend(state: &State<'_, AppState>) -> crate::models::GeometryBackend {
    state.config.lock().unwrap().default_geometry_backend
}

async fn resolve_generation_engine_kind(
    state: &State<'_, AppState>,
    thread_id: Option<&str>,
    explicit: Option<crate::models::EngineKind>,
) -> AppResult<crate::models::EngineKind> {
    if let Some(engine_kind) = explicit {
        return Ok(engine_kind);
    }

    if let Some(thread_id) = thread_id {
        let db = state.db.lock().await;
        if let Some(engine_kind) = db::get_thread_engine_kind(&db, thread_id)
            .map_err(|err| AppError::persistence(err.to_string()))?
        {
            return Ok(engine_kind);
        }
    }

    Ok(default_engine_kind(state))
}

async fn resolve_generation_source_language(
    state: &State<'_, AppState>,
    thread_id: Option<&str>,
    explicit: Option<crate::models::SourceLanguage>,
) -> AppResult<crate::models::SourceLanguage> {
    if let Some(source_language) = explicit {
        return Ok(source_language);
    }

    if let Some(thread_id) = thread_id {
        let db = state.db.lock().await;
        if let Some(source_language) = db::get_thread_source_language(&db, thread_id)
            .map_err(|err| AppError::persistence(err.to_string()))?
        {
            return Ok(source_language);
        }
    }

    Ok(default_source_language(state))
}

async fn resolve_generation_geometry_backend(
    state: &State<'_, AppState>,
    thread_id: Option<&str>,
    explicit: Option<crate::models::GeometryBackend>,
) -> AppResult<crate::models::GeometryBackend> {
    if let Some(geometry_backend) = explicit {
        return Ok(geometry_backend);
    }

    if let Some(thread_id) = thread_id {
        let db = state.db.lock().await;
        if let Some(geometry_backend) = db::get_thread_geometry_backend(&db, thread_id)
            .map_err(|err| AppError::persistence(err.to_string()))?
        {
            return Ok(geometry_backend);
        }
    }

    Ok(default_geometry_backend(state))
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
    let bytes = fs::read(&attachment.path).ok()?;
    let b64 = general_purpose::STANDARD.encode(bytes);
    let ext = attachment
        .path
        .split('.')
        .next_back()
        .unwrap_or("png")
        .to_lowercase();
    let mime = if ext == "jpg" || ext == "jpeg" {
        "image/jpeg"
    } else {
        "image/png"
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
    assets.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
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
                    "No active engine or MCP agent found. Switch to API Key mode in Settings → Agents, or connect an agent."
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
    let engine_kind =
        resolve_generation_engine_kind(&state, thread_id.as_deref(), options.engine_kind).await?;
    let source_language =
        resolve_generation_source_language(&state, thread_id.as_deref(), options.source_language)
            .await?;
    let geometry_backend =
        resolve_generation_geometry_backend(&state, thread_id.as_deref(), options.geometry_backend)
            .await?;
    let question_mode = options.question_mode.unwrap_or(false);
    let follow_up_question = options.follow_up_question;
    let mut ctx = {
        let db = state.db.lock().await;
        crate::context::assemble_context(&db, thread_id, working_design, parent_macro_code)
    };
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
    );
    let contextual_prompt =
        if source_language == crate::models::SourceLanguage::EckyIrV0 && !question_mode {
            format!(
                "{}\n\nEXPERIMENTAL ENGINE TARGET\n{}",
                contextual_prompt,
                ecky_ir_v0_guide_text(geometry_backend)
            )
        } else {
            contextual_prompt
        };
    let contextual_prompt =
        prepend_follow_up_context(contextual_prompt, follow_up_question.as_deref());
    let contextual_prompt =
        if let Some(notes) = build_visual_input_notes(image_data.as_ref(), attachments.as_ref()) {
            format!("{}\n\n{}", contextual_prompt, notes)
        } else {
            contextual_prompt
        };
    let images = prepare_images(image_data, attachments);

    let mut output = llm::generate_design(&engine, &contextual_prompt, images)
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
    let default_engine_kind = default_engine_kind(&state);

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
            let default_source_language = default_source_language(&state);
            let default_geometry_backend = default_geometry_backend(&state);
            db::create_or_update_thread(
                &db,
                &thread_id,
                &initial_title,
                now,
                Some(&traits),
                Some(default_engine_kind),
                Some(default_source_language),
                Some(default_geometry_backend),
            )
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
            let mime = if ext == "jpg" || ext == "jpeg" {
                "image/jpeg"
            } else {
                "image/png"
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
    let bundle = crate::freecad::get_artifact_bundle(&app, &model_id)?;
    let manifest = crate::freecad::get_model_manifest(&app, &model_id)?;
    Ok(crate::services::structural_verification::verify_structure(
        &bundle, &manifest,
    ))
}

#[cfg(test)]
mod tests {
    use super::{prepend_follow_up_context, should_use_framework_for_generation};
    use crate::context::PromptContext;
    use crate::models::{DesignOutput, InteractionMode, MacroDialect, UiSpec};

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
}
