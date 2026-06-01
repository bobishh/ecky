use super::{
    artifact_bundle_digest, carry_forward_semantic_manifest,
    draft_feedback_from_structural_verification, persist_agent_session, settle_live_render_phase,
    store_session_render_preview, try_record_agent_error, AgentContext,
    StoreSessionRenderPreviewRequest,
};
use crate::mcp::contracts::{
    PrintabilityAnalyzeResponse, PrintabilityTransformRecipesGetResponse,
    SemanticTransformArtifactGuard, SemanticTransformPreviewRequest,
    SemanticTransformPreviewResponse,
};
use crate::models::{
    AppError, AppResult, AppState, ArtifactBundle, DesignOutput, FeatureNode, MacroDialect,
    ModelManifest, PathResolver, SourceRef,
};
use crate::services::render;

fn printability_manifest_source_anchor(manifest: &ModelManifest) -> Option<String> {
    let graph = manifest.feature_graph.as_ref()?;
    let anchors = graph
        .nodes
        .iter()
        .filter_map(printability_feature_node_anchor)
        .collect::<Vec<_>>();

    match anchors.as_slice() {
        [anchor] => Some(anchor.clone()),
        _ => None,
    }
}

fn printability_manifest_risk_anchor(
    manifest: &ModelManifest,
) -> Option<crate::services::printability::PrintabilityRiskAnchor> {
    let graph = manifest.feature_graph.as_ref()?;
    let mut anchors = graph
        .nodes
        .iter()
        .filter_map(printability_feature_node_risk_anchor)
        .collect::<Vec<_>>();
    let mut anchor = match anchors.len() {
        1 => anchors.swap_remove(0),
        _ => return None,
    };
    let has_feature_id = anchor
        .feature_id
        .as_ref()
        .is_some_and(|feature_id| !feature_id.trim().is_empty());
    if !has_feature_id {
        return None;
    }
    if anchor.target_ids.is_empty() {
        return Some(anchor);
    }
    anchor.target_ids.dedup();
    anchor.stable_node_keys.dedup();
    Some(anchor)
}

fn printability_feature_node_risk_anchor(
    node: &FeatureNode,
) -> Option<crate::services::printability::PrintabilityRiskAnchor> {
    let feature_id = node.feature_id.trim();
    if feature_id.is_empty() {
        return None;
    }

    let mut target_ids = Vec::new();
    for output_ref in &node.output_refs {
        for target_id in &output_ref.target_ids {
            let trimmed = target_id.trim();
            if !trimmed.is_empty() {
                target_ids.push(trimmed.to_string());
            }
        }
    }
    for port in &node.ports {
        for target_id in &port.target_ids {
            let trimmed = target_id.trim();
            if !trimmed.is_empty() {
                target_ids.push(trimmed.to_string());
            }
        }
    }
    target_ids.dedup();

    let mut stable_node_keys = target_ids
        .iter()
        .filter_map(|target_id| printability_stable_node_key_from_target_id(target_id))
        .collect::<Vec<_>>();
    stable_node_keys.dedup();

    Some(crate::services::printability::PrintabilityRiskAnchor {
        feature_id: Some(feature_id.to_string()),
        target_ids,
        stable_node_keys,
    })
}

fn printability_stable_node_key_from_target_id(target_id: &str) -> Option<String> {
    let (_, remainder) = target_id.split_once(":stable-node-key:")?;
    let (stable_node_key, _) = remainder
        .split_once(":edge:")
        .or_else(|| remainder.split_once(":face:"))?;
    let stable_node_key = stable_node_key.trim();
    (!stable_node_key.is_empty()).then(|| stable_node_key.to_string())
}

fn printability_feature_node_anchor(node: &FeatureNode) -> Option<String> {
    let feature_id = node.feature_id.trim();
    if feature_id.is_empty() {
        return None;
    }

    if let Some(source_anchor) = node
        .source_ref
        .as_ref()
        .and_then(printability_source_ref_anchor)
    {
        return Some(format!("feature:{feature_id}@{source_anchor}"));
    }

    Some(format!("feature:{feature_id}"))
}

fn printability_source_ref_anchor(source_ref: &SourceRef) -> Option<String> {
    let source_id = source_ref
        .source_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let path = source_ref
        .path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let span = match (source_ref.start_byte, source_ref.end_byte) {
        (Some(start), Some(end)) => Some(format!("{start}-{end}")),
        (Some(start), None) => Some(start.to_string()),
        (None, Some(end)) => Some(format!("0-{end}")),
        (None, None) => None,
    };

    let mut parts = Vec::new();
    if let Some(source_id) = source_id {
        parts.push(source_id.to_string());
    }
    if let Some(path) = path {
        parts.push(path.to_string());
    }
    if let Some(span) = span {
        parts.push(span);
    }

    (!parts.is_empty()).then(|| format!("source:{}", parts.join(":")))
}

pub fn handle_printability_analyze(
    _state: &AppState,
    app: &dyn PathResolver,
    thread_id: &str,
    message_id: &str,
    model_id: &str,
) -> AppResult<PrintabilityAnalyzeResponse> {
    let bundle = crate::model_runtime::read_artifact_bundle(app, model_id)?;
    let manifest = crate::model_runtime::read_model_manifest(app, model_id)?;
    let artifact_digest = artifact_bundle_digest(&bundle);
    if bundle.preview_stl_path.trim().is_empty() {
        return Err(AppError::validation(
            "Artifact bundle has no preview STL path.",
        ));
    }
    let mut analysis = crate::services::printability::analyze_stl_path(std::path::Path::new(
        &bundle.preview_stl_path,
    ))
    .map_err(|err| AppError::parse(err.to_string()))?;
    crate::services::printability::enrich_transform_suggestions_with_source_anchor(
        &mut analysis,
        printability_manifest_source_anchor(&manifest),
    );
    crate::services::printability::enrich_transform_suggestions_with_risk_anchor(
        &mut analysis,
        printability_manifest_risk_anchor(&manifest),
    );

    Ok(PrintabilityAnalyzeResponse {
        thread_id: thread_id.to_string(),
        message_id: message_id.to_string(),
        model_id: model_id.to_string(),
        artifact_digest,
        preview_stl_path: bundle.preview_stl_path,
        analysis,
    })
}

pub fn handle_printability_transform_recipes_get(
    _state: &AppState,
    app: &dyn PathResolver,
    thread_id: &str,
    message_id: &str,
    model_id: &str,
) -> AppResult<PrintabilityTransformRecipesGetResponse> {
    let bundle = crate::model_runtime::read_artifact_bundle(app, model_id)?;
    let manifest = crate::model_runtime::read_model_manifest(app, model_id)?;
    let artifact_digest = artifact_bundle_digest(&bundle);
    if bundle.preview_stl_path.trim().is_empty() {
        return Err(AppError::validation(
            "Artifact bundle has no preview STL path.",
        ));
    }
    let mut analysis = crate::services::printability::analyze_stl_path(std::path::Path::new(
        &bundle.preview_stl_path,
    ))
    .map_err(|err| AppError::parse(err.to_string()))?;
    crate::services::printability::enrich_transform_suggestions_with_source_anchor(
        &mut analysis,
        printability_manifest_source_anchor(&manifest),
    );
    crate::services::printability::enrich_transform_suggestions_with_risk_anchor(
        &mut analysis,
        printability_manifest_risk_anchor(&manifest),
    );
    let recipes = crate::services::printability::supportless_fdm_transform_recipes(&analysis);

    Ok(PrintabilityTransformRecipesGetResponse {
        thread_id: thread_id.to_string(),
        message_id: message_id.to_string(),
        model_id: model_id.to_string(),
        artifact_digest,
        preview_stl_path: bundle.preview_stl_path,
        recipes,
    })
}

pub async fn handle_semantic_transform_preview(
    state: &AppState,
    app: &dyn PathResolver,
    req: SemanticTransformPreviewRequest,
    ctx: &AgentContext,
) -> AppResult<SemanticTransformPreviewResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = req.model_id.clone();

    let result = async {
        let conn = state.db.lock().await;
        let target = crate::services::target::resolve_editable_target(
            &conn,
            app,
            req.thread_id.clone(),
            req.message_id.clone(),
        )?;
        drop(conn);

        tracked_thread_id = Some(target.thread_id.clone());
        tracked_message_id = Some(target.message_id.clone());
        let requested_model_id = req
            .model_id
            .clone()
            .or_else(|| target.model_id())
            .ok_or_else(|| AppError::validation("Target has no artifact modelId."))?;
        tracked_model_id = Some(requested_model_id.clone());

        {
            let conn = state.db.lock().await;
            persist_agent_session(
                &conn,
                ctx,
                tracked_thread_id.clone(),
                tracked_message_id.clone(),
                tracked_model_id.clone(),
                "patching_macro",
                "Preparing semantic transform preview.",
            )?;
        }

        let design_output = target.design_output;
        let (bundle, manifest) =
            crate::model_runtime::read_runtime_bundle(app, &requested_model_id)?;
        crate::models::validate_model_runtime_bundle(&manifest, &bundle)?;
        validate_semantic_transform_artifact_guard(&req.expected_artifact, &bundle)?;
        validate_semantic_transform_ecky_source(&design_output, &bundle, &manifest)?;

        match req.action_kind {
            crate::services::printability::SupportlessFdmRecipeActionKind::Reorient => {}
            crate::services::printability::SupportlessFdmRecipeActionKind::Chamfer => {
                return Err(AppError::validation(
                    "semantic_transform_preview actionKind=chamfer is unsupported.",
                ));
            }
            crate::services::printability::SupportlessFdmRecipeActionKind::Split => {
                return Err(AppError::validation(
                    "semantic_transform_preview actionKind=split is unsupported.",
                ));
            }
            crate::services::printability::SupportlessFdmRecipeActionKind::Relief => {
                return Err(AppError::validation(
                    "semantic_transform_preview actionKind=relief is unsupported.",
                ));
            }
            crate::services::printability::SupportlessFdmRecipeActionKind::Clearance => {
                return Err(AppError::validation(
                    "semantic_transform_preview actionKind=clearance is unsupported.",
                ));
            }
        }

        if bundle.preview_stl_path.trim().is_empty() {
            return Err(AppError::validation(
                "Artifact bundle has no preview STL path.",
            ));
        }
        let mut analysis = crate::services::printability::analyze_stl_path(std::path::Path::new(
            &bundle.preview_stl_path,
        ))
        .map_err(|err| AppError::parse(err.to_string()))?;
        crate::services::printability::enrich_transform_suggestions_with_source_anchor(
            &mut analysis,
            printability_manifest_source_anchor(&manifest),
        );
        crate::services::printability::enrich_transform_suggestions_with_risk_anchor(
            &mut analysis,
            printability_manifest_risk_anchor(&manifest),
        );
        let recipes = crate::services::printability::supportless_fdm_transform_recipes(&analysis);
        let recipe = recipes
            .iter()
            .find(|recipe| {
                recipe.recipe_id == req.recipe_id && recipe.action_kind == req.action_kind
            })
            .ok_or_else(|| {
                AppError::validation(format!(
                    "No supportless-FDM recipe matched recipeId={} actionKind={}.",
                    req.recipe_id,
                    semantic_transform_action_kind_label(req.action_kind)
                ))
            })?;
        let rotation_degrees = recipe
            .rotation_degrees
            .ok_or_else(|| AppError::validation("Reorient recipe is missing rotationDegrees."))?;

        let source_digest = crate::mcp::macro_buffer::source_digest(&design_output.macro_code);
        let next_source = crate::services::printability::reorient_ecky_source(
            &design_output.macro_code,
            rotation_degrees,
        )
        .map_err(AppError::validation)?;
        crate::ecky_scheme::compile_to_core_program(&next_source)
            .map_err(|err| AppError::validation(err.to_string()))?;
        let new_source_digest = crate::mcp::macro_buffer::source_digest(&next_source);

        {
            let conn = state.db.lock().await;
            persist_agent_session(
                &conn,
                ctx,
                tracked_thread_id.clone(),
                tracked_message_id.clone(),
                tracked_model_id.clone(),
                "rendering",
                "Rendering semantic transform preview.",
            )?;
        }

        let artifact_bundle = render::render_model_with_previous_manifest(
            &next_source,
            &design_output.initial_params,
            Some(MacroDialect::EckyIrV0),
            Some(design_output.geometry_backend),
            design_output.post_processing.as_ref(),
            Some(&manifest),
            state,
            app,
        )
        .await?;
        let model_manifest =
            crate::model_runtime::read_model_manifest(app, &artifact_bundle.model_id)?;
        let model_manifest =
            carry_forward_semantic_manifest(Some(&manifest), model_manifest, &artifact_bundle);
        let model_manifest = crate::model_runtime::write_model_manifest(
            app,
            &artifact_bundle.model_id,
            &model_manifest,
        )?;
        tracked_model_id = Some(artifact_bundle.model_id.clone());

        let mut preview_design = design_output.clone();
        preview_design.version_name.clear();
        preview_design.response = format!(
            "Draft semantic transform preview for supportless-FDM recipe {}.",
            req.recipe_id
        );
        preview_design.macro_code = next_source;
        preview_design.macro_dialect = MacroDialect::EckyIrV0;
        preview_design.engine_kind = crate::models::EngineKind::EckyIrV0;
        preview_design.source_language = crate::models::SourceLanguage::EckyIrV0;
        preview_design.geometry_backend = artifact_bundle.geometry_backend;

        let sv = crate::services::structural_verification::verify_structure(
            &artifact_bundle,
            &model_manifest,
        );
        let preview = store_session_render_preview(
            state,
            app,
            ctx,
            StoreSessionRenderPreviewRequest {
                thread_id: target.thread_id.clone(),
                base_message_id: Some(target.message_id.clone()),
                design_output: preview_design,
                artifact_bundle: artifact_bundle.clone(),
                model_manifest,
                draft_feedback: Some(draft_feedback_from_structural_verification(&sv)),
            },
        )
        .await?;
        tracked_message_id = Some(preview.preview_id.clone());

        Ok(SemanticTransformPreviewResponse {
            thread_id: target.thread_id,
            base_message_id: target.message_id,
            preview_id: preview.preview_id,
            model_id: artifact_bundle.model_id.clone(),
            recipe_id: recipe.recipe_id.clone(),
            action_kind: recipe.action_kind,
            source_digest,
            new_source_digest,
            preview_support_status: recipe.preview_support_status,
            apply_support_status: recipe.apply_support_status,
            artifact_digest: artifact_bundle_digest(&artifact_bundle),
        })
    }
    .await;

    settle_live_render_phase(
        state,
        ctx,
        tracked_thread_id.as_deref(),
        tracked_message_id.as_deref(),
        tracked_model_id.clone(),
        &result,
    )
    .await;

    if let Err(err) = &result {
        let conn = state.db.lock().await;
        try_record_agent_error(
            state,
            &conn,
            ctx,
            tracked_thread_id,
            tracked_message_id,
            tracked_model_id,
            err,
        );
    }

    result
}

fn validate_semantic_transform_artifact_guard(
    expected: &SemanticTransformArtifactGuard,
    bundle: &ArtifactBundle,
) -> AppResult<()> {
    if expected.model_id != bundle.model_id
        || expected.preview_stl_path != bundle.preview_stl_path
        || expected.content_hash != bundle.content_hash
    {
        return Err(AppError::validation(
            "semantic_transform_preview artifact guard mismatch: expected modelId, previewStlPath, and contentHash must match current runtime bundle.",
        ));
    }
    Ok(())
}

fn validate_semantic_transform_ecky_source(
    design_output: &DesignOutput,
    bundle: &ArtifactBundle,
    manifest: &ModelManifest,
) -> AppResult<()> {
    if design_output.source_language != crate::models::SourceLanguage::EckyIrV0
        || bundle.source_language != crate::models::SourceLanguage::EckyIrV0
        || manifest.source_language != crate::models::SourceLanguage::EckyIrV0
    {
        return Err(AppError::validation(
            "semantic_transform_preview supports sourceLanguage=ecky .ecky source only.",
        ));
    }

    let has_ecky_source_path = bundle
        .macro_path
        .as_deref()
        .and_then(|path| std::path::Path::new(path).extension())
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.eq_ignore_ascii_case("ecky"))
        .unwrap_or(false);
    if !has_ecky_source_path {
        return Err(AppError::validation(
            "semantic_transform_preview supports sourceLanguage=ecky .ecky source only.",
        ));
    }
    Ok(())
}

fn semantic_transform_action_kind_label(
    action_kind: crate::services::printability::SupportlessFdmRecipeActionKind,
) -> &'static str {
    match action_kind {
        crate::services::printability::SupportlessFdmRecipeActionKind::Reorient => "reorient",
        crate::services::printability::SupportlessFdmRecipeActionKind::Chamfer => "chamfer",
        crate::services::printability::SupportlessFdmRecipeActionKind::Split => "split",
        crate::services::printability::SupportlessFdmRecipeActionKind::Relief => "relief",
        crate::services::printability::SupportlessFdmRecipeActionKind::Clearance => "clearance",
    }
}
