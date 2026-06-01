use super::{
    artifact_bundle_digest, carry_forward_semantic_manifest,
    draft_feedback_from_structural_verification, mark_live_session_busy, persist_agent_session,
    push_mcp_profile, push_trace_event_with_conn, session_render_preview_for_request,
    session_target_ref, settle_live_render_phase, store_session_render_preview,
    try_record_agent_error, AgentContext, StoreSessionRenderPreviewRequest, TraceEvent,
};
use crate::mcp::contracts::{
    MacroReplaceRequest, MacroReplaceResponse, ParamsPatchRequest, ParamsPatchResponse,
};
use crate::models::{
    AppError, AppResult, AppState, DesignOutput, InteractionMode, MacroDialect, PathResolver,
    UiSpec,
};
use crate::services::design::{auto_heal_legacy_params, is_param_schema_mismatch};
use crate::services::render;
use std::time::Instant;

pub async fn handle_params_preview_render(
    state: &AppState,
    app: &dyn PathResolver,
    req: ParamsPatchRequest,
    ctx: &AgentContext,
) -> AppResult<ParamsPatchResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = async {
        let conn = state.db.lock().await;
        let preview = session_render_preview_for_request(
            ctx,
            req.thread_id.as_deref(),
            req.message_id.as_deref(),
        );
        let (target_thread_id, target_message_id, base_design, base_model_manifest) =
            if let Some(preview) = preview.clone() {
                (
                    preview.thread_id.clone(),
                    preview
                        .base_message_id
                        .clone()
                        .unwrap_or_else(|| preview.preview_id.clone()),
                    preview.design_output.clone(),
                    Some(preview.model_manifest.clone()),
                )
            } else {
                let target = crate::services::target::resolve_target(
                    &conn,
                    app,
                    req.thread_id.clone(),
                    req.message_id.clone(),
                )?;
                let base_design = target
                    .design
                    .ok_or_else(|| AppError::validation("Target has no design output."))?;
                tracked_model_id = target
                    .artifact_bundle
                    .as_ref()
                    .map(|bundle| bundle.model_id.clone());
                (
                    target.thread_id,
                    target.message_id,
                    base_design,
                    target.model_manifest,
                )
            };

        if let Some(preview) = preview.as_ref() {
            tracked_model_id = Some(preview.artifact_bundle.model_id.clone());
        }
        tracked_thread_id = Some(target_thread_id.clone());
        tracked_message_id = Some(target_message_id.clone());

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "patching_params",
            "",
        )?;
        mark_live_session_busy(
            state,
            ctx,
            Some(session_target_ref(
                target_thread_id.clone(),
                target_message_id.clone(),
                tracked_model_id.clone(),
            )),
            "patching_params",
            Some("Patching parameters for the active target.".to_string()),
            None,
            false,
        )
        .await;
        push_trace_event_with_conn(
            state,
            &conn,
            ctx,
            TraceEvent {
                thread_id: tracked_thread_id.clone(),
                message_id: tracked_message_id.clone(),
                model_id: tracked_model_id.clone(),
                phase: "patching_params",
                kind: "tool_start",
                summary: "Patching parameters for the active target.".to_string(),
                details: None,
            },
        );

        let mut merged_params = base_design.initial_params.clone();
        for (key, value) in req.parameter_patch.clone() {
            merged_params.insert(key, value);
        }

        let mut healed_ui_spec = base_design.ui_spec.clone();
        let mut healed_params = merged_params.clone();
        if let Err(err) = crate::models::validate_design_params(&healed_params, &healed_ui_spec) {
            if base_design.macro_dialect == MacroDialect::Legacy && is_param_schema_mismatch(&err) {
                if let Some((next_ui_spec, next_params, heal_report)) = auto_heal_legacy_params(
                    &base_design.macro_code,
                    &healed_ui_spec,
                    &healed_params,
                    Some(&base_design.initial_params),
                )? {
                    push_trace_event_with_conn(
                        state,
                        &conn,
                        ctx,
                        TraceEvent {
                            thread_id: tracked_thread_id.clone(),
                            message_id: tracked_message_id.clone(),
                            model_id: tracked_model_id.clone(),
                            phase: "patching_params",
                            kind: "auto_heal_applied",
                            summary: "Reconciled legacy uiSpec and initialParams from parsed macro params."
                                .to_string(),
                            details: Some(format!(
                                "added={:?}; dropped={:?}; carried={:?}",
                                heal_report.added_keys, heal_report.dropped_keys, heal_report.carried_keys
                            )),
                        },
                    );
                    healed_ui_spec = next_ui_spec;
                    healed_params = next_params;
                } else {
                    return Err(AppError::with_details(
                        crate::contracts::AppErrorCode::Validation,
                        err.message,
                        format!(
                            "Legacy param auto-heal could not parse dynamic params for session {} on thread {:?}.",
                            ctx.session_id, tracked_thread_id
                        ),
                    ));
                }
            } else {
                return Err(err);
            }
        }

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "rendering",
            "",
        )?;
        mark_live_session_busy(
            state,
            ctx,
            Some(session_target_ref(
                target_thread_id.clone(),
                target_message_id.clone(),
                tracked_model_id.clone(),
            )),
            "rendering",
            Some("Rendering the updated version.".to_string()),
            None,
            false,
        )
        .await;

        let next_post_processing = req
            .post_processing
            .clone()
            .or_else(|| base_design.post_processing.clone());
        let config_default_backend = state.config.lock().unwrap().default_geometry_backend;
        let authoring_context = resolve_macro_authoring_context(
            base_design.source_language,
            base_design.geometry_backend,
            &base_design.macro_dialect,
            req.geometry_backend,
            config_default_backend,
        )?;
        let render_geometry_backend = authoring_context.geometry_backend;
        let base_context = MacroAuthoringContext {
            source_language: base_design.source_language,
            geometry_backend: base_design.geometry_backend,
        };
        log_macro_backend_resolution(
            state,
            &conn,
            ctx,
            "params_preview_render",
            &base_context,
            &base_design.macro_dialect,
            req.geometry_backend,
            &authoring_context,
            Some(&target_thread_id),
            Some(&target_message_id),
            tracked_model_id.as_deref(),
        );

        drop(conn);

        let artifact_bundle = render::render_model_with_previous_manifest(
            &base_design.macro_code,
            &healed_params,
            Some(base_design.macro_dialect.clone()),
            Some(render_geometry_backend),
            next_post_processing.as_ref(),
            base_model_manifest.as_ref(),
            state,
            app,
        )
        .await?;
        let model_manifest =
            crate::model_runtime::read_model_manifest(app, &artifact_bundle.model_id)?;
        let model_manifest = carry_forward_semantic_manifest(
            base_model_manifest.as_ref(),
            model_manifest,
            &artifact_bundle,
        );
        let model_manifest = crate::model_runtime::write_model_manifest(
            app,
            &artifact_bundle.model_id,
            &model_manifest,
        )?;
        tracked_model_id = Some(artifact_bundle.model_id.clone());

        let mut design_output = base_design.clone();
        design_output.ui_spec = healed_ui_spec;
        design_output.initial_params = healed_params.clone();
        design_output.post_processing = next_post_processing;
        design_output.source_language = authoring_context.source_language;
        design_output.geometry_backend = render_geometry_backend;
        design_output.version_name.clear();
        design_output.interaction_mode = InteractionMode::Tune;

        let sv = crate::services::structural_verification::verify_structure(
            &artifact_bundle,
            &model_manifest,
        );
        let preview = store_session_render_preview(
            state,
            app,
            ctx,
            StoreSessionRenderPreviewRequest {
                thread_id: target_thread_id.clone(),
                base_message_id: Some(target_message_id.clone()),
                design_output: design_output.clone(),
                artifact_bundle: artifact_bundle.clone(),
                model_manifest: model_manifest.clone(),
                draft_feedback: Some(draft_feedback_from_structural_verification(&sv)),
            },
        )
        .await?;
        tracked_message_id = Some(preview.preview_id.clone());
        Ok(ParamsPatchResponse {
            thread_id: target_thread_id,
            message_id: preview.preview_id,
            merged_params: healed_params,
            artifact_digest: artifact_bundle_digest(&artifact_bundle),
            artifact_bundle,
            model_manifest,
            design_output,
            structural_verification: Some(sv),
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MacroAuthoringContext {
    pub(super) source_language: crate::models::SourceLanguage,
    pub(super) geometry_backend: crate::models::GeometryBackend,
}

pub(super) fn infer_macro_source_language(dialect: &MacroDialect) -> crate::models::SourceLanguage {
    match dialect {
        MacroDialect::EckyIrV0 => crate::models::SourceLanguage::EckyIrV0,
        MacroDialect::Build123d => crate::models::SourceLanguage::Build123d,
        MacroDialect::Legacy | MacroDialect::CadFrameworkV1 => {
            crate::models::SourceLanguage::LegacyPython
        }
    }
}

fn configured_authoring_context(state: &AppState) -> MacroAuthoringContext {
    let config = state.config.lock().unwrap();
    MacroAuthoringContext {
        source_language: config.default_source_language,
        geometry_backend: config.default_geometry_backend,
    }
}

fn log_macro_backend_resolution(
    state: &AppState,
    conn: &rusqlite::Connection,
    ctx: &AgentContext,
    phase: &'static str,
    base_context: &MacroAuthoringContext,
    macro_dialect: &MacroDialect,
    requested_geometry_backend: Option<crate::models::GeometryBackend>,
    resolved_context: &MacroAuthoringContext,
    thread_id: Option<&str>,
    message_id: Option<&str>,
    model_id: Option<&str>,
) {
    let configured_context = configured_authoring_context(state);
    let requested = requested_geometry_backend
        .map(|backend| backend.as_str())
        .unwrap_or("none");
    let summary = format!(
        "Resolved macro render backend: sourceLanguage={} geometryBackend={}.",
        resolved_context.source_language.as_str(),
        resolved_context.geometry_backend.as_str()
    );
    let details = format!(
        "baseSourceLanguage={}; baseGeometryBackend={}; requestedGeometryBackend={}; configSourceLanguage={}; configGeometryBackend={}; macroDialect={:?}",
        base_context.source_language.as_str(),
        base_context.geometry_backend.as_str(),
        requested,
        configured_context.source_language.as_str(),
        configured_context.geometry_backend.as_str(),
        macro_dialect,
    );
    eprintln!(
        "[MCP] {} backend_resolved session={} agent={} thread={} message={} model={} {} {}",
        phase,
        ctx.session_id,
        ctx.agent_label,
        thread_id.unwrap_or("-"),
        message_id.unwrap_or("-"),
        model_id.unwrap_or("-"),
        summary,
        details,
    );
    push_trace_event_with_conn(
        state,
        conn,
        ctx,
        TraceEvent {
            thread_id: thread_id.map(str::to_string),
            message_id: message_id.map(str::to_string),
            model_id: model_id.map(str::to_string),
            phase,
            kind: "backend_resolved",
            summary,
            details: Some(details),
        },
    );
}

pub(super) fn resolve_macro_authoring_context(
    base_source_language: crate::models::SourceLanguage,
    base_geometry_backend: crate::models::GeometryBackend,
    macro_dialect: &MacroDialect,
    requested_geometry_backend: Option<crate::models::GeometryBackend>,
    config_default_backend: crate::models::GeometryBackend,
) -> AppResult<MacroAuthoringContext> {
    let macro_source_language = infer_macro_source_language(macro_dialect);
    if macro_source_language != base_source_language {
        return Err(AppError::validation(format!(
            "Macro source language mismatch: target model is {}, macro is {}. Fork or create a new version before migrating source language.",
            base_source_language.as_str(),
            macro_source_language.as_str()
        )));
    }

    if let Some(requested) = requested_geometry_backend {
        if base_source_language != crate::models::SourceLanguage::EckyIrV0
            && requested != base_geometry_backend
        {
            return Err(AppError::validation(format!(
                "Geometry backend override is only valid for Ecky source. Target model is {} on {}; requested backend is {}.",
                base_source_language.as_str(),
                base_geometry_backend.as_str(),
                requested.as_str()
            )));
        }
    }

    // Global config owns the geometry engine for Ecky source. An explicit
    // per-render request still wins, but otherwise the current config default
    // applies regardless of which backend last rendered this version —
    // switching the config engine re-renders every Ecky model on it, no new
    // thread needed. Non-Ecky source (legacy python / build123d) is bound to
    // its own backend and cannot be switched by config.
    let geometry_backend = if base_source_language == crate::models::SourceLanguage::EckyIrV0 {
        requested_geometry_backend.unwrap_or(config_default_backend)
    } else {
        base_geometry_backend
    };

    Ok(MacroAuthoringContext {
        source_language: base_source_language,
        geometry_backend,
    })
}

pub(super) fn first_version_authoring_context(
    state: &AppState,
    macro_dialect: &MacroDialect,
    requested_geometry_backend: Option<crate::models::GeometryBackend>,
) -> MacroAuthoringContext {
    match infer_macro_source_language(macro_dialect) {
        crate::models::SourceLanguage::LegacyPython => MacroAuthoringContext {
            source_language: crate::models::SourceLanguage::EckyIrV0,
            geometry_backend: requested_geometry_backend.unwrap_or_else(|| {
                let fallback = configured_authoring_context(state);
                fallback.geometry_backend
            }),
        },
        crate::models::SourceLanguage::Build123d => MacroAuthoringContext {
            source_language: crate::models::SourceLanguage::Build123d,
            geometry_backend: crate::models::GeometryBackend::Build123d,
        },
        crate::models::SourceLanguage::EckyIrV0 => {
            let fallback = configured_authoring_context(state);
            MacroAuthoringContext {
                source_language: crate::models::SourceLanguage::EckyIrV0,
                geometry_backend: requested_geometry_backend.unwrap_or(fallback.geometry_backend),
            }
        }
    }
}

pub async fn handle_macro_preview_render(
    state: &AppState,
    app: &dyn PathResolver,
    req: MacroReplaceRequest,
    ctx: &AgentContext,
) -> AppResult<MacroReplaceResponse> {
    let total_started = Instant::now();
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = async {
        let (working_thread_id, base_design, base_model_manifest) = if let Some(preview) =
            session_render_preview_for_request(
                ctx,
                req.thread_id.as_deref(),
                req.message_id.as_deref(),
            )
        {
            tracked_thread_id = Some(preview.thread_id.clone());
            tracked_message_id = preview
                .base_message_id
                .clone()
                .or_else(|| Some(preview.preview_id.clone()));
            tracked_model_id = Some(preview.artifact_bundle.model_id.clone());
            (
                preview.thread_id,
                preview.design_output,
                Some(preview.model_manifest),
            )
        } else if req.message_id.is_some() {
            let conn = state.db.lock().await;
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

            let base_design = target
                .design
                .ok_or_else(|| AppError::validation("Target has no design output."))?;
            (target.thread_id, base_design, target.model_manifest)
        } else {
            let thread_id = req.thread_id.clone().ok_or_else(|| {
                AppError::validation("thread_id is required to create the first version.")
            })?;
            tracked_thread_id = Some(thread_id.clone());
            let stub = DesignOutput {
                title: String::new(),
                version_name: String::new(),
                response: String::new(),
                interaction_mode: InteractionMode::Design,
                macro_code: String::new(),
                macro_dialect: MacroDialect::Legacy,
                engine_kind: crate::models::EngineKind::default(),
                source_language: crate::models::SourceLanguage::default(),
                geometry_backend: crate::models::GeometryBackend::default(),
                ui_spec: UiSpec { fields: vec![] },
                initial_params: std::collections::BTreeMap::new(),
                post_processing: None,
            };
            (thread_id, stub, None)
        };

        let conn = state.db.lock().await;

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "patching_macro",
            "",
        )?;
        mark_live_session_busy(
            state,
            ctx,
            tracked_thread_id
                .as_ref()
                .zip(tracked_message_id.as_ref())
                .map(|(t, m)| session_target_ref(t.clone(), m.clone(), tracked_model_id.clone())),
            "patching_macro",
            Some("Replacing macro code for the active target.".to_string()),
            None,
            false,
        )
        .await;
        push_trace_event_with_conn(
            state,
            &conn,
            ctx,
            TraceEvent {
                thread_id: tracked_thread_id.clone(),
                message_id: tracked_message_id.clone(),
                model_id: tracked_model_id.clone(),
                phase: "patching_macro",
                kind: "tool_start",
                summary: "Replacing macro code for the active target.".to_string(),
                details: None,
            },
        );

        let requested_macro_dialect = req
            .macro_dialect
            .clone()
            .unwrap_or_else(|| crate::contracts::infer_macro_dialect_from_code(&req.macro_code));
        let is_ir = requested_macro_dialect == MacroDialect::EckyIrV0;
        let framework_parsed = if requested_macro_dialect == MacroDialect::CadFrameworkV1 {
            crate::commands::design::derive_framework_controls(&req.macro_code)?
        } else if requested_macro_dialect == MacroDialect::Legacy {
            crate::commands::design::derive_framework_controls(&req.macro_code)?
        } else {
            None
        };
        let parsed_legacy = if framework_parsed.is_none()
            && requested_macro_dialect != MacroDialect::Build123d
        {
            Some(crate::commands::design::parse_macro_params(req.macro_code.clone()))
        } else {
            None
        };
        let macro_edit_parameter_source = if base_design.macro_code.trim().is_empty() {
            req.parameters
                .clone()
                .unwrap_or_else(|| base_design.initial_params.clone())
        } else {
            base_design.initial_params.clone()
        };
        let (mut ui_spec, mut initial_params, macro_dialect) =
            if let Some(parsed) = framework_parsed {
                (
                    UiSpec {
                        fields: parsed.fields.clone(),
                    },
                    crate::commands::design::reconcile_framework_params(
                        &parsed.fields,
                        &macro_edit_parameter_source,
                        &parsed.params,
                    ),
                    MacroDialect::CadFrameworkV1,
                )
            } else if is_ir {
                let parsed = parsed_legacy
                    .clone()
                    .expect("parse_macro_params should exist for IR path");
                let params = crate::commands::design::reconcile_framework_params(
                    &parsed.fields,
                    &macro_edit_parameter_source,
                    &parsed.params,
                );
                (
                    req.ui_spec.clone().unwrap_or(UiSpec {
                        fields: parsed.fields,
                    }),
                    params,
                    MacroDialect::EckyIrV0,
                )
            } else if requested_macro_dialect == MacroDialect::Build123d {
                (
                    req.ui_spec
                        .clone()
                        .unwrap_or_else(|| base_design.ui_spec.clone()),
                    macro_edit_parameter_source.clone(),
                    MacroDialect::Build123d,
                )
            } else {
                let parsed_legacy = parsed_legacy
                    .clone()
                    .expect("legacy parser should exist when framework parse is absent");
                let mut reconciled_params = parsed_legacy.params.clone();
                for (key, value) in &macro_edit_parameter_source {
                    if reconciled_params.contains_key(key.as_str()) {
                        reconciled_params.insert(key.clone(), value.clone());
                    }
                }
                (
                    req.ui_spec.clone().unwrap_or_else(|| {
                        if parsed_legacy.fields.is_empty() {
                            base_design.ui_spec.clone()
                        } else {
                            UiSpec {
                                fields: parsed_legacy.fields,
                            }
                        }
                    }),
                    reconciled_params,
                    MacroDialect::Legacy,
                )
            };
        if let Err(err) = crate::models::validate_design_params(&initial_params, &ui_spec) {
            if macro_dialect == MacroDialect::Legacy && is_param_schema_mismatch(&err) {
                if let Some((next_ui_spec, next_params, heal_report)) = auto_heal_legacy_params(
                    &req.macro_code,
                    &ui_spec,
                    &initial_params,
                    Some(&base_design.initial_params),
                )? {
                    push_trace_event_with_conn(
                        state,
                        &conn,
                        ctx,
                        TraceEvent {
                            thread_id: tracked_thread_id.clone(),
                            message_id: tracked_message_id.clone(),
                            model_id: tracked_model_id.clone(),
                            phase: "patching_macro",
                            kind: "auto_heal_applied",
                            summary: "Reconciled legacy uiSpec and initialParams from parsed macro params."
                                .to_string(),
                            details: Some(format!(
                                "added={:?}; dropped={:?}; carried={:?}",
                                heal_report.added_keys, heal_report.dropped_keys, heal_report.carried_keys
                            )),
                        },
                    );
                    ui_spec = next_ui_spec;
                    initial_params = next_params;
                } else {
                    let parsed_keys = parsed_legacy
                        .as_ref()
                        .map(|parsed| parsed.params.keys().cloned().collect::<Vec<_>>())
                        .unwrap_or_default();
                    return Err(AppError::with_details(
                        crate::contracts::AppErrorCode::Validation,
                        err.message,
                        format!(
                            "Legacy param auto-heal failed for session {} on thread {:?}. parsedKeys={:?}",
                            ctx.session_id, tracked_thread_id, parsed_keys
                        ),
                    ));
                }
            } else {
                return Err(err);
            }
        }

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "rendering",
            "",
        )?;
        mark_live_session_busy(
            state,
            ctx,
            tracked_thread_id
                .as_ref()
                .zip(tracked_message_id.as_ref())
                .map(|(t, m)| session_target_ref(t.clone(), m.clone(), tracked_model_id.clone())),
            "rendering",
            Some("Rendering the updated version.".to_string()),
            None,
            false,
        )
        .await;

        let base_context = if base_design.macro_code.trim().is_empty() {
            first_version_authoring_context(state, &macro_dialect, req.geometry_backend)
        } else {
            MacroAuthoringContext {
                source_language: base_design.source_language,
                geometry_backend: base_design.geometry_backend,
            }
        };
        let config_default_backend = state.config.lock().unwrap().default_geometry_backend;
        let authoring_context = resolve_macro_authoring_context(
            base_context.source_language,
            base_context.geometry_backend,
            &macro_dialect,
            req.geometry_backend,
            config_default_backend,
        )?;
        let render_geometry_backend = authoring_context.geometry_backend;
        log_macro_backend_resolution(
            state,
            &conn,
            ctx,
            "macro_preview_render",
            &base_context,
            &macro_dialect,
            req.geometry_backend,
            &authoring_context,
            Some(&working_thread_id),
            tracked_message_id.as_deref(),
            tracked_model_id.as_deref(),
        );

        drop(conn);

        let next_post_processing = req
            .post_processing
            .clone()
            .or_else(|| base_design.post_processing.clone());

        let render_started = Instant::now();
        let artifact_bundle = render::render_model_with_previous_manifest(
            &req.macro_code,
            &initial_params,
            Some(macro_dialect.clone()),
            Some(render_geometry_backend),
            next_post_processing.as_ref(),
            base_model_manifest.as_ref(),
            state,
            app,
        )
        .await?;
        push_mcp_profile(
            state,
            ctx,
            "macro_preview_render",
            "render_model",
            render_started,
            Some(&working_thread_id),
            tracked_message_id.as_deref(),
            Some(&artifact_bundle.model_id),
        );
        let manifest_started = Instant::now();
        let model_manifest =
            crate::model_runtime::read_model_manifest(app, &artifact_bundle.model_id)?;
        let model_manifest = carry_forward_semantic_manifest(
            base_model_manifest.as_ref(),
            model_manifest,
            &artifact_bundle,
        );
        let model_manifest = crate::model_runtime::write_model_manifest(
            app,
            &artifact_bundle.model_id,
            &model_manifest,
        )?;
        tracked_model_id = Some(artifact_bundle.model_id.clone());
        push_mcp_profile(
            state,
            ctx,
            "macro_preview_render",
            "manifest_read_carry_write",
            manifest_started,
            Some(&working_thread_id),
            tracked_message_id.as_deref(),
            tracked_model_id.as_deref(),
        );

        let engine_kind = authoring_context.source_language.to_engine_kind();
        let design_output = DesignOutput {
            title: base_design.title.clone(),
            version_name: String::new(),
            response: "Draft update via macro replacement.".to_string(),
            interaction_mode: InteractionMode::Design,
            macro_code: req.macro_code.clone(),
            macro_dialect,
            engine_kind,
            source_language: authoring_context.source_language,
            geometry_backend: render_geometry_backend,
            ui_spec: ui_spec.clone(),
            initial_params: initial_params.clone(),
            post_processing: next_post_processing,
        };

        let sv = crate::services::structural_verification::verify_structure(
            &artifact_bundle,
            &model_manifest,
        );
        let store_started = Instant::now();
        let preview = store_session_render_preview(
            state,
            app,
            ctx,
            StoreSessionRenderPreviewRequest {
                thread_id: working_thread_id.clone(),
                base_message_id: tracked_message_id.clone(),
                design_output: design_output.clone(),
                artifact_bundle: artifact_bundle.clone(),
                model_manifest: model_manifest.clone(),
                draft_feedback: Some(draft_feedback_from_structural_verification(&sv)),
            },
        )
        .await?;
        push_mcp_profile(
            state,
            ctx,
            "macro_preview_render",
            "store_preview",
            store_started,
            Some(&working_thread_id),
            Some(&preview.preview_id),
            Some(&artifact_bundle.model_id),
        );
        tracked_message_id = Some(preview.preview_id.clone());
        Ok(MacroReplaceResponse {
            thread_id: working_thread_id,
            message_id: preview.preview_id,
            macro_code: req.macro_code.clone(),
            ui_spec,
            initial_params,
            artifact_digest: artifact_bundle_digest(&artifact_bundle),
            artifact_bundle,
            model_manifest,
            structural_verification: Some(sv),
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

    push_mcp_profile(
        state,
        ctx,
        "macro_preview_render",
        if result.is_ok() {
            "total_ok"
        } else {
            "total_err"
        },
        total_started,
        tracked_thread_id.as_deref(),
        tracked_message_id.as_deref(),
        tracked_model_id.as_deref(),
    );

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
