use super::{
    artifact_bundle_digest, macro_buffer_digest, macro_buffer_line_window, macro_buffer_lines,
    map_target_resolved_from, persist_agent_session, session_render_preview_for_request,
    try_record_agent_error, AgentContext,
};
use crate::mcp::contracts::{
    TargetGetRequest, TargetGetResponse, TargetMacroRequest, TargetMacroResponse,
    TargetMetaRequest, TargetMetaResponse, TargetResolvedFrom,
};
use crate::models::{
    AppError, AppResult, AppState, ArtifactBundle, DesignOutput, ModelManifest, PathResolver,
    SourceLanguage, WorkspaceSceneLens, WorkspaceSceneRepresentation,
    WorkspaceSceneRepresentationKind, WorkspaceSceneRepresentationStatus, WorkspaceSceneTopology,
};

pub async fn handle_target_get(
    state: &AppState,
    app: &dyn PathResolver,
    req: TargetGetRequest,
    ctx: &AgentContext,
) -> AppResult<TargetGetResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let conn = state.db.lock().await;

    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = (|| -> AppResult<TargetGetResponse> {
        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            None,
            "reading",
            "",
        )?;

        let target = crate::services::target::resolve_target(
            &conn,
            app,
            req.thread_id.clone(),
            req.message_id.clone(),
        )?;

        tracked_thread_id = Some(target.thread_id.clone());
        tracked_message_id = Some(target.message_id.clone());
        tracked_model_id = target
            .artifact_bundle
            .as_ref()
            .map(|bundle| bundle.model_id.clone());

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "reading",
            "",
        )?;

        let design = target
            .design
            .ok_or_else(|| AppError::validation("Target has no design output."))?;

        let artifact_digest = target.artifact_bundle.as_ref().map(artifact_bundle_digest);

        Ok(TargetGetResponse {
            thread_id: target.thread_id,
            message_id: target.message_id,
            title: design.title,
            version_name: design.version_name,
            macro_code: design.macro_code,
            ui_spec: design.ui_spec,
            initial_params: design.initial_params,
            artifact_bundle: target.artifact_bundle,
            artifact_digest,
            model_manifest: target.model_manifest,
            latest_draft: None,
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
fn build_agent_scene_packet(
    design_output: &DesignOutput,
    artifact_bundle: Option<&ArtifactBundle>,
    model_manifest: Option<&ModelManifest>,
    has_draft: bool,
) -> crate::models::AgentScenePacket {
    let has_source = !design_output.macro_code.trim().is_empty();
    let exact_committed = artifact_bundle.is_some() && model_manifest.is_some();
    let sketch_status = if has_source {
        WorkspaceSceneRepresentationStatus::Rebuildable
    } else {
        WorkspaceSceneRepresentationStatus::Pending
    };
    let draft_status = if has_draft {
        WorkspaceSceneRepresentationStatus::Fresh
    } else if has_source {
        WorkspaceSceneRepresentationStatus::Stale
    } else {
        WorkspaceSceneRepresentationStatus::Pending
    };
    let exact_status = if exact_committed {
        WorkspaceSceneRepresentationStatus::Committed
    } else if has_source {
        WorkspaceSceneRepresentationStatus::Rebuildable
    } else {
        WorkspaceSceneRepresentationStatus::Pending
    };
    let active_lens = if exact_status == WorkspaceSceneRepresentationStatus::Committed {
        WorkspaceSceneLens::Exact
    } else if draft_status == WorkspaceSceneRepresentationStatus::Fresh {
        WorkspaceSceneLens::Draft
    } else {
        WorkspaceSceneLens::Sketch
    };
    let mut allowed_patch_targets = Vec::new();
    if has_source {
        allowed_patch_targets.push("macroBufferReplaceAndPreview".to_string());
    }
    if design_output.source_language == SourceLanguage::EckyIrV0 {
        allowed_patch_targets.push("eckyAstReplaceAndRender".to_string());
    }
    if model_manifest.is_some() {
        allowed_patch_targets.push("semanticManifestPatch".to_string());
    }
    if has_draft {
        allowed_patch_targets.push("commitPreviewVersion".to_string());
    }

    crate::models::AgentScenePacket {
        schema_version: 1,
        active_lens,
        representations: vec![
            WorkspaceSceneRepresentation {
                kind: WorkspaceSceneRepresentationKind::SketchIntent,
                status: sketch_status,
            },
            WorkspaceSceneRepresentation {
                kind: WorkspaceSceneRepresentationKind::MeshDraft,
                status: draft_status,
            },
            WorkspaceSceneRepresentation {
                kind: WorkspaceSceneRepresentationKind::ExactModel,
                status: exact_status,
            },
        ],
        topology: WorkspaceSceneTopology {
            edge_target_count: artifact_bundle
                .map(|bundle| bundle.edge_targets.len())
                .unwrap_or(0),
            face_target_count: artifact_bundle
                .map(|bundle| bundle.face_targets.len())
                .unwrap_or(0),
            selection_target_count: model_manifest
                .map(|manifest| manifest.selection_targets.len())
                .unwrap_or(0),
            control_primitive_count: model_manifest
                .map(|manifest| manifest.control_primitives.len())
                .unwrap_or(0),
            control_relation_count: model_manifest
                .map(|manifest| manifest.control_relations.len())
                .unwrap_or(0),
            control_view_count: model_manifest
                .map(|manifest| manifest.control_views.len())
                .unwrap_or(0),
        },
        allowed_patch_targets,
    }
}

#[allow(dead_code)]
fn build_target_meta_response(
    target: &crate::services::target::EditableTarget,
) -> TargetMetaResponse {
    let (range_count, number_count, select_count, checkbox_count) = target
        .design_output
        .ui_spec
        .fields
        .iter()
        .fold((0, 0, 0, 0), |acc, field| match field {
            crate::models::UiField::Range { .. } => (acc.0 + 1, acc.1, acc.2, acc.3),
            crate::models::UiField::Number { .. } => (acc.0, acc.1 + 1, acc.2, acc.3),
            crate::models::UiField::Select { .. } => (acc.0, acc.1, acc.2 + 1, acc.3),
            crate::models::UiField::Checkbox { .. } => (acc.0, acc.1, acc.2, acc.3 + 1),
            crate::models::UiField::Image { .. } => acc,
        });

    let export_formats = target
        .artifact_bundle
        .as_ref()
        .map(|bundle| {
            bundle
                .export_artifacts
                .iter()
                .map(|artifact| artifact.format.as_str().to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let has_step_export = export_formats
        .iter()
        .any(|format| format.eq_ignore_ascii_case("step"));
    let step_export_path = target.artifact_bundle.as_ref().and_then(|bundle| {
        bundle
            .export_artifacts
            .iter()
            .find(|artifact| artifact.format.eq_ignore_ascii_case("step"))
            .map(|artifact| artifact.path.clone())
    });
    let edge_target_count = target
        .artifact_bundle
        .as_ref()
        .map(|bundle| bundle.edge_targets.len())
        .unwrap_or(0);
    let face_target_count = target
        .artifact_bundle
        .as_ref()
        .map(|bundle| bundle.face_targets.len())
        .unwrap_or(0);

    TargetMetaResponse {
        thread_id: target.thread_id.clone(),
        message_id: target.message_id.clone(),
        title: target.design_output.title.clone(),
        version_name: target.design_output.version_name.clone(),
        model_id: target.model_id(),
        source_language: target.design_output.source_language.as_str().to_string(),
        macro_dialect: crate::mcp::authoring::macro_dialect_label(
            &target.design_output.macro_dialect,
        )
        .to_string(),
        geometry_backend: target.design_output.geometry_backend.as_str().to_string(),
        has_draft: false,
        resolved_from: map_target_resolved_from(target.resolved_from),
        has_artifact_bundle: target.artifact_bundle.is_some(),
        has_runtime_manifest: target.artifact_bundle.is_some() && target.model_manifest.is_some(),
        export_formats,
        has_step_export,
        step_export_path,
        edge_target_count,
        face_target_count,
        ui_field_count: target.design_output.ui_spec.fields.len(),
        range_count,
        number_count,
        select_count,
        checkbox_count,
        parameter_count: target.design_output.initial_params.len(),
        has_semantic_manifest: target.model_manifest.is_some(),
        control_primitive_count: target
            .model_manifest
            .as_ref()
            .map(|manifest| manifest.control_primitives.len())
            .unwrap_or(0),
        control_relation_count: target
            .model_manifest
            .as_ref()
            .map(|manifest| manifest.control_relations.len())
            .unwrap_or(0),
        control_view_count: target
            .model_manifest
            .as_ref()
            .map(|manifest| manifest.control_views.len())
            .unwrap_or(0),
        scene_packet: build_agent_scene_packet(
            &target.design_output,
            target.artifact_bundle.as_ref(),
            target.model_manifest.as_ref(),
            false,
        ),
    }
}

pub async fn handle_target_meta_get(
    state: &AppState,
    app: &dyn PathResolver,
    req: TargetMetaRequest,
    ctx: &AgentContext,
) -> AppResult<TargetMetaResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let conn = state.db.lock().await;

    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = (|| -> AppResult<TargetMetaResponse> {
        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            None,
            "reading",
            "Reading target metadata.",
        )?;

        let preview = session_render_preview_for_request(
            ctx,
            req.thread_id.as_deref(),
            req.message_id.as_deref(),
        );
        let has_draft = preview.is_some();
        let (target_thread_id, target_message_id, design_output, artifact_bundle, model_manifest) =
            if let Some(preview) = preview {
                (
                    preview.thread_id,
                    preview.preview_id,
                    preview.design_output,
                    Some(preview.artifact_bundle),
                    Some(preview.model_manifest),
                )
            } else {
                let target = crate::services::target::resolve_editable_target(
                    &conn,
                    app,
                    req.thread_id.clone(),
                    req.message_id.clone(),
                )?;
                (
                    target.thread_id,
                    target.message_id,
                    target.design_output,
                    target.artifact_bundle,
                    target.model_manifest,
                )
            };

        tracked_thread_id = Some(target_thread_id.clone());
        tracked_message_id = Some(target_message_id.clone());
        tracked_model_id = artifact_bundle
            .as_ref()
            .map(|bundle| bundle.model_id.clone());

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "reading",
            "",
        )?;

        let (range_count, number_count, select_count, checkbox_count) = design_output
            .ui_spec
            .fields
            .iter()
            .fold((0, 0, 0, 0), |acc, field| match field {
                crate::models::UiField::Range { .. } => (acc.0 + 1, acc.1, acc.2, acc.3),
                crate::models::UiField::Number { .. } => (acc.0, acc.1 + 1, acc.2, acc.3),
                crate::models::UiField::Select { .. } => (acc.0, acc.1, acc.2 + 1, acc.3),
                crate::models::UiField::Checkbox { .. } => (acc.0, acc.1, acc.2, acc.3 + 1),
                crate::models::UiField::Image { .. } => acc,
            });
        let export_formats = artifact_bundle
            .as_ref()
            .map(|bundle| {
                bundle
                    .export_artifacts
                    .iter()
                    .map(|artifact| artifact.format.as_str().to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let has_step_export = export_formats
            .iter()
            .any(|format| format.eq_ignore_ascii_case("step"));
        let step_export_path = artifact_bundle.as_ref().and_then(|bundle| {
            bundle
                .export_artifacts
                .iter()
                .find(|artifact| artifact.format.eq_ignore_ascii_case("step"))
                .map(|artifact| artifact.path.clone())
        });
        let edge_target_count = artifact_bundle
            .as_ref()
            .map(|bundle| bundle.edge_targets.len())
            .unwrap_or(0);
        let face_target_count = artifact_bundle
            .as_ref()
            .map(|bundle| bundle.face_targets.len())
            .unwrap_or(0);
        let scene_packet = build_agent_scene_packet(
            &design_output,
            artifact_bundle.as_ref(),
            model_manifest.as_ref(),
            has_draft,
        );

        Ok(TargetMetaResponse {
            thread_id: target_thread_id,
            message_id: target_message_id,
            title: design_output.title,
            version_name: design_output.version_name,
            model_id: artifact_bundle
                .as_ref()
                .map(|bundle| bundle.model_id.clone()),
            source_language: design_output.source_language.as_str().to_string(),
            macro_dialect: crate::mcp::authoring::macro_dialect_label(&design_output.macro_dialect)
                .to_string(),
            geometry_backend: design_output.geometry_backend.as_str().to_string(),
            has_draft,
            resolved_from: TargetResolvedFrom::Base,
            has_artifact_bundle: artifact_bundle.is_some(),
            has_runtime_manifest: artifact_bundle.is_some() && model_manifest.is_some(),
            export_formats,
            has_step_export,
            step_export_path,
            edge_target_count,
            face_target_count,
            ui_field_count: design_output.ui_spec.fields.len(),
            range_count,
            number_count,
            select_count,
            checkbox_count,
            parameter_count: design_output.initial_params.len(),
            has_semantic_manifest: model_manifest.is_some(),
            control_primitive_count: model_manifest
                .as_ref()
                .map(|manifest| manifest.control_primitives.len())
                .unwrap_or(0),
            control_relation_count: model_manifest
                .as_ref()
                .map(|manifest| manifest.control_relations.len())
                .unwrap_or(0),
            control_view_count: model_manifest
                .as_ref()
                .map(|manifest| manifest.control_views.len())
                .unwrap_or(0),
            scene_packet,
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

pub async fn handle_target_macro_get(
    state: &AppState,
    app: &dyn PathResolver,
    req: TargetMacroRequest,
    ctx: &AgentContext,
) -> AppResult<TargetMacroResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let conn = state.db.lock().await;

    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = (|| -> AppResult<TargetMacroResponse> {
        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            None,
            "reading",
            "Reading target macro.",
        )?;

        let preview = session_render_preview_for_request(
            ctx,
            req.thread_id.as_deref(),
            req.message_id.as_deref(),
        );
        let (target_thread_id, target_message_id, design_output, artifact_bundle, _model_manifest) =
            if let Some(preview) = preview {
                (
                    preview.thread_id,
                    preview.preview_id,
                    preview.design_output,
                    Some(preview.artifact_bundle),
                    Some(preview.model_manifest),
                )
            } else {
                let target = crate::services::target::resolve_editable_target(
                    &conn,
                    app,
                    req.thread_id.clone(),
                    req.message_id.clone(),
                )?;
                (
                    target.thread_id,
                    target.message_id,
                    target.design_output,
                    target.artifact_bundle,
                    target.model_manifest,
                )
            };

        tracked_thread_id = Some(target_thread_id.clone());
        tracked_message_id = Some(target_message_id.clone());
        tracked_model_id = artifact_bundle
            .as_ref()
            .map(|bundle| bundle.model_id.clone());

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "reading",
            "",
        )?;

        let authoring_context = crate::mcp::authoring::target_authoring_context(&design_output);
        let artifact_digest = artifact_bundle.as_ref().map(artifact_bundle_digest);
        let macro_code = design_output.macro_code;
        let lines = macro_buffer_lines(&macro_code);
        let line_count = lines.len();
        let digest = macro_buffer_digest(&macro_code);
        let (window_start_line, window_end_line, truncated, window_lines) =
            macro_buffer_line_window(&lines, req.start_line, req.end_line)?;

        Ok(TargetMacroResponse {
            thread_id: target_thread_id,
            message_id: target_message_id,
            title: design_output.title,
            version_name: design_output.version_name,
            resolved_from: TargetResolvedFrom::Base,
            digest,
            line_count,
            window_start_line,
            window_end_line,
            truncated,
            lines: window_lines,
            macro_dialect: design_output.macro_dialect,
            post_processing: design_output.post_processing,
            authoring_context,
            artifact_digest,
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
