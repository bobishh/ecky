use super::{
    artifact_bundle_digest, ensure_turn_working_version_message, now_secs, persist_agent_session,
    try_record_agent_error, AgentContext,
};
use crate::mcp::contracts::{
    ControlPrimitiveDeleteRequest, ControlPrimitiveSaveRequest, ControlViewDeleteRequest,
    ControlViewSaveRequest, MeasurementAnnotationDeleteRequest, MeasurementAnnotationSaveRequest,
    SemanticManifestDetailRequest, SemanticManifestDetailResponse,
    SemanticManifestMutationResponse, SemanticManifestRequest, SemanticManifestResponse,
    SemanticManifestSection,
};
use crate::models::{
    AppError, AppResult, AppState, ArtifactBundle, ControlPrimitive, ControlView,
    ControlViewSource, DesignOutput, MeasurementAnnotation, MeasurementAnnotationSource,
    ModelManifest, ModelSourceKind, PathResolver,
};
use crate::services::agent_versions::{
    save_or_update_agent_version_for_session, SaveOrUpdateAgentVersionRequest,
};

async fn resolve_turn_working_semantic_target(
    state: &AppState,
    app: &dyn PathResolver,
    ctx: &AgentContext,
    target: SemanticTargetState,
    create_summary: String,
) -> AppResult<SemanticTargetState> {
    let working_message_id = ensure_turn_working_version_message(
        state,
        app,
        ctx,
        &target.thread_id,
        &target.message_id,
        Some(target.artifact_bundle.model_id.clone()),
        &target.design_output,
        Some(target.artifact_bundle.clone()),
        Some(target.model_manifest.clone()),
        create_summary,
    )
    .await?;
    let conn = state.db.lock().await;
    resolve_semantic_target(&conn, app, Some(target.thread_id), Some(working_message_id))
}

#[derive(Debug, Clone)]
struct SemanticTargetState {
    thread_id: String,
    message_id: String,
    design_output: DesignOutput,
    artifact_bundle: ArtifactBundle,
    model_manifest: ModelManifest,
}

fn resolve_semantic_target(
    conn: &rusqlite::Connection,
    app: &dyn PathResolver,
    thread_id: Option<String>,
    message_id: Option<String>,
) -> AppResult<SemanticTargetState> {
    let target =
        crate::services::target::resolve_editable_target(conn, app, thread_id, message_id)?;
    let design_output = target.design_output;
    let artifact_bundle = target
        .artifact_bundle
        .ok_or_else(|| AppError::validation("Target has no artifact bundle."))?;
    let model_manifest = target
        .model_manifest
        .ok_or_else(|| AppError::validation("Target has no model manifest."))?;

    if model_manifest.source_kind != ModelSourceKind::Generated {
        return Err(AppError::validation(
            "Semantic knob/view MCP tools currently support generated models only.",
        ));
    }

    crate::models::validate_model_runtime_bundle(&model_manifest, &artifact_bundle)?;

    Ok(SemanticTargetState {
        thread_id: target.thread_id,
        message_id: target.message_id,
        design_output,
        artifact_bundle,
        model_manifest,
    })
}

#[allow(clippy::too_many_arguments)]
async fn save_semantic_manifest_version(
    state: &AppState,
    app: &dyn PathResolver,
    ctx: &AgentContext,
    target: SemanticTargetState,
    next_manifest: ModelManifest,
    title: Option<String>,
    version_name: Option<String>,
    response_text: String,
) -> AppResult<SemanticManifestMutationResponse> {
    crate::models::validate_model_runtime_bundle(&next_manifest, &target.artifact_bundle)?;

    let mut design_output = target.design_output.clone();
    if let Some(next_title) = title.clone() {
        design_output.title = next_title;
    }
    if let Some(next_version_name) = version_name.clone() {
        design_output.version_name = next_version_name;
    } else {
        design_output.version_name.clear();
    }

    let save_result = save_or_update_agent_version_for_session(
        state,
        app,
        SaveOrUpdateAgentVersionRequest {
            session_id: ctx.session_id.clone(),
            thread_id: target.thread_id.clone(),
            base_message_id: target.message_id.clone(),
            model_id: Some(target.artifact_bundle.model_id.clone()),
            design_output: design_output.clone(),
            artifact_bundle: Some(target.artifact_bundle.clone()),
            model_manifest: Some(next_manifest.clone()),
            updated_at: now_secs(),
            response_text_created: response_text.clone(),
            response_text_updated: response_text,
            preserve_existing_title: title.is_none(),
            preserve_existing_version_name: version_name.is_none(),
            force_create_new_message: false,
            announce_created_working_version: false,
        },
    )
    .await?;
    let agent_origin = save_result.agent_origin.clone();
    let artifact_digest = artifact_bundle_digest(&target.artifact_bundle);

    Ok(SemanticManifestMutationResponse {
        thread_id: target.thread_id,
        message_id: save_result.message_id,
        model_id: target.artifact_bundle.model_id.clone(),
        title: design_output.title,
        version_name: save_result.version_name,
        artifact_digest,
        control_primitive_count: next_manifest.control_primitives.len(),
        relation_count: next_manifest.control_relations.len(),
        view_count: next_manifest.control_views.len(),
        advisory_count: next_manifest.advisories.len(),
        measurement_annotation_count: next_manifest.measurement_annotations.len(),
        part_count: next_manifest.parts.len(),
        agent_origin,
    })
}

fn normalize_llm_primitive(
    primitive: ControlPrimitive,
    existing: Option<&ControlPrimitive>,
    manifest: &ModelManifest,
) -> AppResult<ControlPrimitive> {
    let primitive_id = primitive.primitive_id.trim();
    if primitive_id.is_empty() {
        return Err(AppError::validation("Primitive id cannot be empty."));
    }

    let order = if primitive.order == 0 {
        existing.map(|value| value.order).unwrap_or_else(|| {
            manifest
                .control_primitives
                .iter()
                .map(|entry| entry.order)
                .max()
                .unwrap_or(0)
                + 1
        })
    } else {
        primitive.order
    };

    Ok(ControlPrimitive {
        primitive_id: primitive_id.to_string(),
        label: primitive.label.trim().to_string(),
        kind: primitive.kind,
        source: ControlViewSource::Llm,
        part_ids: primitive.part_ids,
        bindings: primitive.bindings,
        editable: primitive.editable,
        order,
    })
}

fn normalize_llm_view(
    view: ControlView,
    existing: Option<&ControlView>,
    manifest: &ModelManifest,
) -> AppResult<ControlView> {
    let view_id = view.view_id.trim();
    if view_id.is_empty() {
        return Err(AppError::validation("View id cannot be empty."));
    }

    let order = if view.order == 0 {
        existing.map(|value| value.order).unwrap_or_else(|| {
            manifest
                .control_views
                .iter()
                .map(|entry| entry.order)
                .max()
                .unwrap_or(0)
                + 1
        })
    } else {
        view.order
    };

    Ok(ControlView {
        view_id: view_id.to_string(),
        label: view.label.trim().to_string(),
        scope: view.scope,
        part_ids: view.part_ids,
        primitive_ids: view.primitive_ids,
        sections: view.sections,
        is_default: view.is_default,
        source: ControlViewSource::Llm,
        status: view.status,
        order,
    })
}

fn normalize_llm_measurement_annotation(
    annotation: MeasurementAnnotation,
) -> AppResult<MeasurementAnnotation> {
    let annotation_id = annotation.annotation_id.trim();
    if annotation_id.is_empty() {
        return Err(AppError::validation(
            "Measurement annotation id cannot be empty.",
        ));
    }

    let label = annotation.label.trim();
    if label.is_empty() {
        return Err(AppError::validation(
            "Measurement annotation label cannot be empty.",
        ));
    }

    Ok(MeasurementAnnotation {
        annotation_id: annotation_id.to_string(),
        label: label.to_string(),
        basis: annotation.basis,
        axis: annotation.axis,
        parameter_keys: annotation.parameter_keys,
        primitive_ids: annotation.primitive_ids,
        target_ids: annotation.target_ids,
        guide_id: annotation.guide_id.and_then(|value| {
            let trimmed = value.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        }),
        explanation: annotation.explanation.and_then(|value| {
            let trimmed = value.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        }),
        formula_hint: annotation.formula_hint.and_then(|value| {
            let trimmed = value.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        }),
        source: MeasurementAnnotationSource::Llm,
    })
}

pub async fn handle_semantic_manifest_get(
    state: &AppState,
    app: &dyn PathResolver,
    req: SemanticManifestRequest,
    ctx: &AgentContext,
) -> AppResult<SemanticManifestResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let conn = state.db.lock().await;
    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = (|| -> AppResult<SemanticManifestResponse> {
        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            None,
            "reading",
            "Reading semantic manifest summary.",
        )?;

        let target =
            resolve_semantic_target(&conn, app, req.thread_id.clone(), req.message_id.clone())?;

        tracked_thread_id = Some(target.thread_id.clone());
        tracked_message_id = Some(target.message_id.clone());
        tracked_model_id = Some(target.artifact_bundle.model_id.clone());

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "idle",
            "",
        )?;

        Ok(SemanticManifestResponse {
            thread_id: target.thread_id,
            message_id: target.message_id,
            title: Some(target.design_output.title),
            version_name: Some(target.design_output.version_name),
            control_primitive_count: target.model_manifest.control_primitives.len(),
            relation_count: target.model_manifest.control_relations.len(),
            view_count: target.model_manifest.control_views.len(),
            advisory_count: target.model_manifest.advisories.len(),
            measurement_annotation_count: target.model_manifest.measurement_annotations.len(),
            part_count: target.model_manifest.parts.len(),
        })
    })();

    if let Err(err) = &result {
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

pub async fn handle_semantic_manifest_detail_get(
    state: &AppState,
    app: &dyn PathResolver,
    req: SemanticManifestDetailRequest,
    ctx: &AgentContext,
) -> AppResult<SemanticManifestDetailResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let conn = state.db.lock().await;
    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = (|| -> AppResult<SemanticManifestDetailResponse> {
        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            None,
            "reading",
            format!(
                "Reading semantic manifest detail section {:?}.",
                req.section
            ),
        )?;

        let target =
            resolve_semantic_target(&conn, app, req.thread_id.clone(), req.message_id.clone())?;

        tracked_thread_id = Some(target.thread_id.clone());
        tracked_message_id = Some(target.message_id.clone());
        tracked_model_id = Some(target.artifact_bundle.model_id.clone());

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "idle",
            "",
        )?;

        let (
            control_primitives,
            control_relations,
            control_views,
            advisories,
            measurement_annotations,
            parts,
        ) = match req.section {
            SemanticManifestSection::ControlPrimitives => (
                Some(target.model_manifest.control_primitives),
                None,
                None,
                None,
                None,
                None,
            ),
            SemanticManifestSection::ControlRelations => (
                None,
                Some(target.model_manifest.control_relations),
                None,
                None,
                None,
                None,
            ),
            SemanticManifestSection::ControlViews => (
                None,
                None,
                Some(target.model_manifest.control_views),
                None,
                None,
                None,
            ),
            SemanticManifestSection::Advisories => (
                None,
                None,
                None,
                Some(target.model_manifest.advisories),
                None,
                None,
            ),
            SemanticManifestSection::MeasurementAnnotations => (
                None,
                None,
                None,
                None,
                Some(target.model_manifest.measurement_annotations),
                None,
            ),
            SemanticManifestSection::Parts => (
                None,
                None,
                None,
                None,
                None,
                Some(target.model_manifest.parts),
            ),
        };

        Ok(SemanticManifestDetailResponse {
            thread_id: target.thread_id,
            message_id: target.message_id,
            section: req.section,
            control_primitives,
            control_relations,
            control_views,
            advisories,
            measurement_annotations,
            parts,
        })
    })();

    if let Err(err) = &result {
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

pub async fn handle_control_primitive_save(
    state: &AppState,
    app: &dyn PathResolver,
    req: ControlPrimitiveSaveRequest,
    ctx: &AgentContext,
) -> AppResult<SemanticManifestMutationResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = async {
        let conn = state.db.lock().await;
        let target =
            resolve_semantic_target(&conn, app, req.thread_id.clone(), req.message_id.clone())?;
        drop(conn);
        let target = resolve_turn_working_semantic_target(
            state,
            app,
            ctx,
            target,
            format!(
                "{} created a working version for this turn.",
                ctx.agent_label
            ),
        )
        .await?;
        let conn = state.db.lock().await;
        tracked_thread_id = Some(target.thread_id.clone());
        tracked_message_id = Some(target.message_id.clone());
        tracked_model_id = Some(target.artifact_bundle.model_id.clone());

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "saving_version",
            "Saving semantic knob.",
        )?;

        let existing = target
            .model_manifest
            .control_primitives
            .iter()
            .find(|entry| entry.primitive_id == req.primitive.primitive_id);
        let next_primitive =
            normalize_llm_primitive(req.primitive, existing, &target.model_manifest)?;
        let next_primitive_id = next_primitive.primitive_id.clone();
        let mut next_manifest = target.model_manifest.clone();
        next_manifest.control_primitives = next_manifest
            .control_primitives
            .into_iter()
            .filter(|entry| entry.primitive_id != next_primitive_id)
            .chain(std::iter::once(next_primitive))
            .collect();
        next_manifest.control_primitives.sort_by(|left, right| {
            left.order
                .cmp(&right.order)
                .then_with(|| left.label.cmp(&right.label))
        });

        drop(conn);

        let response = save_semantic_manifest_version(
            state,
            app,
            ctx,
            target,
            next_manifest,
            req.title,
            req.version_name,
            format!("{} updated a semantic knob via MCP.", ctx.agent_label),
        )
        .await?;
        tracked_message_id = Some(response.message_id.clone());
        tracked_model_id = Some(response.model_id.clone());

        Ok(response)
    }
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

pub async fn handle_control_primitive_delete(
    state: &AppState,
    app: &dyn PathResolver,
    req: ControlPrimitiveDeleteRequest,
    ctx: &AgentContext,
) -> AppResult<SemanticManifestMutationResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = async {
        let conn = state.db.lock().await;
        let target =
            resolve_semantic_target(&conn, app, req.thread_id.clone(), req.message_id.clone())?;
        drop(conn);
        let target = resolve_turn_working_semantic_target(
            state,
            app,
            ctx,
            target,
            format!(
                "{} created a working version for this turn.",
                ctx.agent_label
            ),
        )
        .await?;
        let conn = state.db.lock().await;
        tracked_thread_id = Some(target.thread_id.clone());
        tracked_message_id = Some(target.message_id.clone());
        tracked_model_id = Some(target.artifact_bundle.model_id.clone());

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "saving_version",
            "Deleting semantic knob.",
        )?;

        let mut next_manifest = target.model_manifest.clone();
        let primitive_id = req.primitive_id;
        next_manifest
            .control_primitives
            .retain(|entry| entry.primitive_id != primitive_id);
        next_manifest.control_relations.retain(|relation| {
            relation.source_primitive_id != primitive_id
                && relation.target_primitive_id != primitive_id
        });
        for view in &mut next_manifest.control_views {
            view.primitive_ids.retain(|entry| entry != &primitive_id);
            for section in &mut view.sections {
                section.primitive_ids.retain(|entry| entry != &primitive_id);
            }
        }
        for advisory in &mut next_manifest.advisories {
            advisory
                .primitive_ids
                .retain(|entry| entry != &primitive_id);
        }

        drop(conn);

        let response = save_semantic_manifest_version(
            state,
            app,
            ctx,
            target,
            next_manifest,
            req.title,
            req.version_name,
            format!("{} deleted a semantic knob via MCP.", ctx.agent_label),
        )
        .await?;
        tracked_message_id = Some(response.message_id.clone());
        tracked_model_id = Some(response.model_id.clone());

        Ok(response)
    }
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

pub async fn handle_control_view_save(
    state: &AppState,
    app: &dyn PathResolver,
    req: ControlViewSaveRequest,
    ctx: &AgentContext,
) -> AppResult<SemanticManifestMutationResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = async {
        let conn = state.db.lock().await;
        let target =
            resolve_semantic_target(&conn, app, req.thread_id.clone(), req.message_id.clone())?;
        drop(conn);
        let target = resolve_turn_working_semantic_target(
            state,
            app,
            ctx,
            target,
            format!(
                "{} created a working version for this turn.",
                ctx.agent_label
            ),
        )
        .await?;
        let conn = state.db.lock().await;
        tracked_thread_id = Some(target.thread_id.clone());
        tracked_message_id = Some(target.message_id.clone());
        tracked_model_id = Some(target.artifact_bundle.model_id.clone());

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "saving_version",
            "Saving semantic view.",
        )?;

        let existing = target
            .model_manifest
            .control_views
            .iter()
            .find(|entry| entry.view_id == req.view.view_id);
        let next_view = normalize_llm_view(req.view, existing, &target.model_manifest)?;
        let next_view_id = next_view.view_id.clone();
        let mut next_manifest = target.model_manifest.clone();
        next_manifest.control_views = next_manifest
            .control_views
            .into_iter()
            .filter(|entry| entry.view_id != next_view_id)
            .chain(std::iter::once(next_view))
            .collect();
        next_manifest.control_views.sort_by(|left, right| {
            left.order
                .cmp(&right.order)
                .then_with(|| left.label.cmp(&right.label))
        });

        drop(conn);

        let response = save_semantic_manifest_version(
            state,
            app,
            ctx,
            target,
            next_manifest,
            req.title,
            req.version_name,
            format!("{} updated a semantic view via MCP.", ctx.agent_label),
        )
        .await?;
        tracked_message_id = Some(response.message_id.clone());
        tracked_model_id = Some(response.model_id.clone());

        Ok(response)
    }
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

pub async fn handle_control_view_delete(
    state: &AppState,
    app: &dyn PathResolver,
    req: ControlViewDeleteRequest,
    ctx: &AgentContext,
) -> AppResult<SemanticManifestMutationResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = async {
        let conn = state.db.lock().await;
        let target =
            resolve_semantic_target(&conn, app, req.thread_id.clone(), req.message_id.clone())?;
        drop(conn);
        let target = resolve_turn_working_semantic_target(
            state,
            app,
            ctx,
            target,
            format!(
                "{} created a working version for this turn.",
                ctx.agent_label
            ),
        )
        .await?;
        let conn = state.db.lock().await;
        tracked_thread_id = Some(target.thread_id.clone());
        tracked_message_id = Some(target.message_id.clone());
        tracked_model_id = Some(target.artifact_bundle.model_id.clone());

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "saving_version",
            "Deleting semantic view.",
        )?;

        let mut next_manifest = target.model_manifest.clone();
        let view_id = req.view_id;
        next_manifest
            .control_views
            .retain(|entry| entry.view_id != view_id);
        for advisory in &mut next_manifest.advisories {
            advisory.view_ids.retain(|entry| entry != &view_id);
        }

        drop(conn);

        let response = save_semantic_manifest_version(
            state,
            app,
            ctx,
            target,
            next_manifest,
            req.title,
            req.version_name,
            format!("{} deleted a semantic view via MCP.", ctx.agent_label),
        )
        .await?;
        tracked_message_id = Some(response.message_id.clone());
        tracked_model_id = Some(response.model_id.clone());

        Ok(response)
    }
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

pub async fn handle_measurement_annotation_save(
    state: &AppState,
    app: &dyn PathResolver,
    req: MeasurementAnnotationSaveRequest,
    ctx: &AgentContext,
) -> AppResult<SemanticManifestMutationResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = async {
        let conn = state.db.lock().await;
        let target =
            resolve_semantic_target(&conn, app, req.thread_id.clone(), req.message_id.clone())?;
        drop(conn);
        let target = resolve_turn_working_semantic_target(
            state,
            app,
            ctx,
            target,
            format!(
                "{} created a working version for this turn.",
                ctx.agent_label
            ),
        )
        .await?;
        let conn = state.db.lock().await;
        tracked_thread_id = Some(target.thread_id.clone());
        tracked_message_id = Some(target.message_id.clone());
        tracked_model_id = Some(target.artifact_bundle.model_id.clone());

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "saving_version",
            "Saving measurement annotation.",
        )?;

        let next_annotation = normalize_llm_measurement_annotation(req.annotation)?;
        let next_annotation_id = next_annotation.annotation_id.clone();
        let mut next_manifest = target.model_manifest.clone();
        if let Some(existing) = next_manifest
            .measurement_annotations
            .iter_mut()
            .find(|entry| entry.annotation_id == next_annotation_id)
        {
            *existing = next_annotation;
        } else {
            next_manifest.measurement_annotations.push(next_annotation);
        }

        drop(conn);

        let response = save_semantic_manifest_version(
            state,
            app,
            ctx,
            target,
            next_manifest,
            req.title,
            req.version_name,
            format!(
                "{} updated a measurement annotation via MCP.",
                ctx.agent_label
            ),
        )
        .await?;
        tracked_message_id = Some(response.message_id.clone());
        tracked_model_id = Some(response.model_id.clone());

        Ok(response)
    }
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

pub async fn handle_measurement_annotation_delete(
    state: &AppState,
    app: &dyn PathResolver,
    req: MeasurementAnnotationDeleteRequest,
    ctx: &AgentContext,
) -> AppResult<SemanticManifestMutationResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = async {
        let conn = state.db.lock().await;
        let target =
            resolve_semantic_target(&conn, app, req.thread_id.clone(), req.message_id.clone())?;
        tracked_thread_id = Some(target.thread_id.clone());
        tracked_message_id = Some(target.message_id.clone());
        tracked_model_id = Some(target.artifact_bundle.model_id.clone());

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "saving_version",
            "Deleting measurement annotation.",
        )?;

        let mut next_manifest = target.model_manifest.clone();
        let annotation_id = req.annotation_id;
        next_manifest
            .measurement_annotations
            .retain(|entry| entry.annotation_id != annotation_id);

        drop(conn);

        let response = save_semantic_manifest_version(
            state,
            app,
            ctx,
            target,
            next_manifest,
            req.title,
            req.version_name,
            format!(
                "{} deleted a measurement annotation via MCP.",
                ctx.agent_label
            ),
        )
        .await?;
        tracked_message_id = Some(response.message_id.clone());
        tracked_model_id = Some(response.model_id.clone());

        Ok(response)
    }
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
