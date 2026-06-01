use super::*;

pub async fn handle_ecky_ast_get(
    state: &AppState,
    app: &dyn PathResolver,
    req: EckyAstGetRequest,
    ctx: &AgentContext,
) -> AppResult<EckyAstGetResponse> {
    if !ecky_ast_authoring_enabled(state) {
        return Err(AppError::validation(
            "Ecky AST authoring is disabled. Set mcp.eckyAstAuthoring=true to expose ecky_ast_get.",
        ));
    }

    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let conn = state.db.lock().await;

    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = (|| -> AppResult<EckyAstGetResponse> {
        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            None,
            "reading",
            "Reading Ecky AST.",
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

        if design_output.source_language != crate::models::SourceLanguage::EckyIrV0 {
            return Err(AppError::validation(format!(
                "ecky_ast_get only supports sourceLanguage=ecky. Target sourceLanguage={}.",
                design_output.source_language.as_str()
            )));
        }

        let source = design_output.macro_code.clone();
        let program = crate::ecky_scheme::compile_to_core_program(&source)
            .map_err(|err| AppError::validation(format!("Failed to compile Ecky source: {err}")))?;
        let depth = req
            .depth
            .unwrap_or(DEFAULT_ECKY_AST_DEPTH)
            .min(MAX_ECKY_AST_DEPTH);
        let max_nodes = req
            .max_nodes
            .unwrap_or(DEFAULT_ECKY_AST_MAX_NODES)
            .clamp(1, MAX_ECKY_AST_NODES);
        let requested_path = req.path.filter(|path| !path.trim().is_empty());
        let root_paths = program
            .parameters
            .iter()
            .map(|param| format!("/params/{}", path_segment(&param.key)))
            .chain(
                program
                    .parts
                    .iter()
                    .map(|part| format!("/parts/{}/root", path_segment(&part.key))),
            )
            .chain(
                program
                    .parts
                    .iter()
                    .map(|part| format!("/parts/{}", path_segment(&part.key))),
            )
            .collect::<Vec<_>>();
        let mut nodes = Vec::new();
        let mut truncated = false;
        truncated |= collect_core_param_ast_nodes(
            &program,
            &source,
            requested_path.as_deref(),
            max_nodes,
            &mut nodes,
        )?;
        truncated |= collect_core_part_clause_ast_nodes(
            &program,
            &source,
            requested_path.as_deref(),
            max_nodes,
            &mut nodes,
        )?;

        for part in &program.parts {
            if nodes.len() >= max_nodes {
                truncated = true;
                break;
            }
            let root_path = format!("/parts/{}/root", path_segment(&part.key));
            if let Some(requested_path) = requested_path.as_deref() {
                if requested_path == "/" {
                    truncated |= collect_core_ast_nodes(
                        &source,
                        &part.root,
                        &root_path,
                        Some(&part.key),
                        depth,
                        max_nodes,
                        &mut nodes,
                    );
                } else if requested_path.starts_with(&root_path) {
                    truncated |= collect_matching_core_ast_nodes(
                        &source,
                        &part.root,
                        &root_path,
                        Some(&part.key),
                        requested_path,
                        depth,
                        max_nodes,
                        &mut nodes,
                    );
                }
            } else {
                truncated |= collect_core_ast_nodes(
                    &source,
                    &part.root,
                    &root_path,
                    Some(&part.key),
                    depth,
                    max_nodes,
                    &mut nodes,
                );
            }
            if nodes.len() >= max_nodes {
                truncated = true;
                break;
            }
        }

        if requested_path.as_deref().is_some_and(|path| path != "/") && nodes.is_empty() {
            return Err(AppError::validation(format!(
                "Ecky AST path not found: {}.",
                requested_path.as_deref().unwrap_or_default()
            )));
        }

        if req.include_source.unwrap_or(false) {
            attach_ecky_ast_source_slices(&source, &mut nodes);
        }

        let core_digest = crate::mcp::macro_buffer::source_digest(
            &nodes
                .iter()
                .map(|node| format!("{}={}", node.path, node.digest))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let authoring_context = crate::mcp::authoring::target_authoring_context(&design_output);
        let artifact_digest = artifact_bundle.as_ref().map(artifact_bundle_digest);

        Ok(EckyAstGetResponse {
            thread_id: target_thread_id,
            message_id: target_message_id,
            title: design_output.title,
            version_name: design_output.version_name,
            resolved_from: TargetResolvedFrom::Base,
            source_digest: crate::mcp::macro_buffer::source_digest(&source),
            core_digest,
            root_paths,
            requested_path,
            depth,
            max_nodes,
            truncated,
            nodes,
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

pub async fn handle_ecky_dependency_get(
    state: &AppState,
    app: &dyn PathResolver,
    req: EckyDependencyGetRequest,
    ctx: &AgentContext,
) -> AppResult<EckyDependencyGetResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let conn = state.db.lock().await;

    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = (|| -> AppResult<EckyDependencyGetResponse> {
        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            None,
            "reading",
            "Reading Ecky dependency graph.",
        )?;

        let preview = session_render_preview_for_request(
            ctx,
            req.thread_id.as_deref(),
            req.message_id.as_deref(),
        );
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

        if design_output.source_language != crate::models::SourceLanguage::EckyIrV0 {
            return Err(AppError::validation(format!(
                "ecky_dependency_get only supports sourceLanguage=ecky. Target sourceLanguage={}.",
                design_output.source_language.as_str()
            )));
        }

        let path = req.path.trim();
        if path.is_empty() {
            return Err(AppError::validation(
                "ecky_dependency_get requires path. Supported path shapes: /params/{key}, /targets/{targetId}.",
            ));
        }

        let source = design_output.macro_code.clone();
        let program = crate::ecky_scheme::compile_to_core_program(&source)
            .map_err(|err| AppError::validation(format!("Failed to compile Ecky source: {err}")))?;
        let query = parse_ecky_dependency_path(path)?;

        let (
            dependency_kind,
            dependent_source_paths,
            impacted_part_ids,
            impact_labels,
            feature_ids,
            parameter_keys,
            target_ids,
        ) = match query {
            EckyDependencyQuery::ParameterKey(param_key) => {
                let param_id = param_id_for_dependency_key(&program, &param_key)?;
                let dependent_source_paths = dependent_source_paths_for_param(&program, param_id);
                let reference_count = dependent_source_paths.len();
                let impacted_part_ids =
                    impacted_part_ids_for_dependency_paths(&dependent_source_paths);
                let impact_labels =
                    impact_labels_for_dependency(&impacted_part_ids, reference_count);
                (
                    "parameterReference".to_string(),
                    dependent_source_paths,
                    impacted_part_ids,
                    impact_labels,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                )
            }
            EckyDependencyQuery::SelectionTargetId(target_id) => {
                let manifest = model_manifest.as_ref().ok_or_else(|| {
                    AppError::validation(
                        "ecky_dependency_get /targets/{targetId} requires a target modelManifest.",
                    )
                })?;
                let target = selection_target_by_id(manifest, &target_id).ok_or_else(|| {
                    AppError::validation(format!(
                        "Ecky dependency source path not found: /targets/{}.",
                        target_id
                    ))
                })?;

                let target_ids = selection_target_match_ids(target);
                let parameter_keys = target.parameter_keys.clone();
                let impacted_part_ids = vec![target.part_id.clone()];
                let (feature_ids, dependent_source_paths) =
                    feature_bindings_for_target_ids(manifest, &target_ids);
                let impact_labels =
                    impact_labels_for_dependency(&impacted_part_ids, dependent_source_paths.len());
                (
                    "selectionTargetReference".to_string(),
                    dependent_source_paths,
                    impacted_part_ids,
                    impact_labels,
                    feature_ids,
                    parameter_keys,
                    target_ids,
                )
            }
        };
        let reference_count = dependent_source_paths.len();
        let authoring_context = crate::mcp::authoring::target_authoring_context(&design_output);
        let artifact_digest = artifact_bundle.as_ref().map(artifact_bundle_digest);

        Ok(EckyDependencyGetResponse {
            thread_id: target_thread_id,
            message_id: target_message_id,
            title: design_output.title,
            version_name: design_output.version_name,
            resolved_from: TargetResolvedFrom::Base,
            source_digest: crate::mcp::macro_buffer::source_digest(&source),
            path: path.to_string(),
            dependency_kind,
            dependent_source_paths,
            impacted_part_ids,
            impact_labels,
            feature_ids,
            parameter_keys,
            target_ids,
            reference_count,
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

pub async fn handle_ecky_selector_resolve(
    state: &AppState,
    app: &dyn PathResolver,
    req: EckySelectorResolveRequest,
    ctx: &AgentContext,
) -> AppResult<EckySelectorResolveResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let conn = state.db.lock().await;

    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = (|| -> AppResult<EckySelectorResolveResponse> {
        let requested_target_id = req.target_id.trim();
        if requested_target_id.is_empty() {
            return Err(AppError::validation(
                "ecky_selector_resolve requires targetId.",
            ));
        }

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            None,
            "reading",
            "Resolving selector target.",
        )?;

        let target = crate::services::target::resolve_editable_target(
            &conn,
            app,
            req.thread_id.clone(),
            req.message_id.clone(),
        )?;

        tracked_thread_id = Some(target.thread_id.clone());
        tracked_message_id = Some(target.message_id.clone());
        tracked_model_id = target.model_id();

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "reading",
            "",
        )?;

        let Some(manifest) = target.model_manifest.as_ref() else {
            return Ok(EckySelectorResolveResponse {
                target_id: requested_target_id.to_string(),
                durable_target_id: None,
                canonical_target_id: None,
                feature_ids: Vec::new(),
                parameter_keys: Vec::new(),
                provenance_candidates: EckySelectorResolveProvenanceCandidates {
                    feature_role: None,
                    source_stable_node_keys: Vec::new(),
                    operation_kinds: Vec::new(),
                    primitive_ids: Vec::new(),
                },
                confidence: EckySelectorResolveConfidence::None,
                reason: "No model manifest available for active target.".to_string(),
            });
        };

        let matched_targets = selection_targets_by_id(manifest, requested_target_id);
        if matched_targets.is_empty() {
            return Ok(EckySelectorResolveResponse {
                target_id: requested_target_id.to_string(),
                durable_target_id: None,
                canonical_target_id: None,
                feature_ids: Vec::new(),
                parameter_keys: Vec::new(),
                provenance_candidates: EckySelectorResolveProvenanceCandidates {
                    feature_role: None,
                    source_stable_node_keys: Vec::new(),
                    operation_kinds: Vec::new(),
                    primitive_ids: Vec::new(),
                },
                confidence: EckySelectorResolveConfidence::None,
                reason: format!(
                    "No selection target matched targetId `{}`.",
                    requested_target_id
                ),
            });
        }

        if matched_targets.len() > 1 {
            let mut feature_ids = Vec::new();
            let mut parameter_keys = Vec::new();
            for matched in &matched_targets {
                push_unique_strings(&mut parameter_keys, &matched.parameter_keys);
                let (target_feature_ids, _) =
                    feature_bindings_for_target_ids(manifest, &selection_target_match_ids(matched));
                push_unique_strings(&mut feature_ids, &target_feature_ids);
            }
            let provenance_candidates = collect_selector_provenance_candidates(
                manifest,
                &matched_targets,
                Some(&target.design_output.macro_code),
            );
            return Ok(EckySelectorResolveResponse {
                target_id: requested_target_id.to_string(),
                durable_target_id: None,
                canonical_target_id: None,
                feature_ids,
                parameter_keys,
                provenance_candidates,
                confidence: EckySelectorResolveConfidence::Ambiguous,
                reason: format!(
                    "Alias collision: {} selection targets matched targetId `{}`.",
                    matched_targets.len(),
                    requested_target_id
                ),
            });
        }

        let selected = matched_targets[0];
        let resolved_target_id = selected
            .target_id
            .clone()
            .unwrap_or_else(|| requested_target_id.to_string());
        let parameter_keys = selected.parameter_keys.clone();
        let (feature_ids, _) =
            feature_bindings_for_target_ids(manifest, &selection_target_match_ids(selected));
        let provenance_candidates = collect_selector_provenance_candidates(
            manifest,
            &[selected],
            Some(&target.design_output.macro_code),
        );

        let (confidence, reason) = if feature_ids.len() > 1 {
            (
                EckySelectorResolveConfidence::Ambiguous,
                format!(
                    "Multiple feature matches ({}) found for targetId `{}`.",
                    feature_ids.len(),
                    requested_target_id
                ),
            )
        } else if !parameter_keys.is_empty() {
            (
                EckySelectorResolveConfidence::Exact,
                "Resolved single selection target with <=1 feature match and non-empty parameter keys."
                    .to_string(),
            )
        } else {
            (
                EckySelectorResolveConfidence::Inferred,
                "Resolved target, but feature/parameter binding is partial.".to_string(),
            )
        };

        Ok(EckySelectorResolveResponse {
            target_id: resolved_target_id,
            durable_target_id: selected.durable_target_id.clone(),
            canonical_target_id: selected.canonical_target_id.clone(),
            feature_ids,
            parameter_keys,
            provenance_candidates,
            confidence,
            reason,
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

pub async fn handle_ecky_constraints_validate(
    state: &AppState,
    app: &dyn PathResolver,
    req: EckyConstraintsValidateRequest,
    ctx: &AgentContext,
) -> AppResult<EckyConstraintsValidateResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let conn = state.db.lock().await;

    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = (|| -> AppResult<EckyConstraintsValidateResponse> {
        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            None,
            "reading",
            "Validating Ecky parameter constraints.",
        )?;

        let preview = session_render_preview_for_request(
            ctx,
            req.thread_id.as_deref(),
            req.message_id.as_deref(),
        );
        let (target_thread_id, target_message_id, design_output, artifact_bundle) =
            if let Some(preview) = preview {
                (
                    preview.thread_id,
                    preview.preview_id,
                    preview.design_output,
                    Some(preview.artifact_bundle),
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

        if design_output.source_language != crate::models::SourceLanguage::EckyIrV0 {
            return Err(AppError::validation(format!(
                "ecky_constraints_validate only supports sourceLanguage=ecky. Target sourceLanguage={}.",
                design_output.source_language.as_str()
            )));
        }

        let source = design_output.macro_code.clone();
        let program = crate::ecky_scheme::compile_to_core_program(&source)
            .map_err(|err| AppError::validation(format!("Failed to compile Ecky source: {err}")))?;
        let (params, parameter_source) = effective_ecky_constraint_params(
            &program,
            &design_output.initial_params,
            req.parameters,
        );
        let rows = validate_ecky_constraints(&source, &program, &params);
        let authoring_lints = collect_ecky_constraint_authoring_lints(&source, &program);
        let pass_count = rows.iter().filter(|row| row.status == "pass").count();
        let fail_count = rows.len().saturating_sub(pass_count);
        let authoring_context = crate::mcp::authoring::target_authoring_context(&design_output);
        let artifact_digest = artifact_bundle.as_ref().map(artifact_bundle_digest);

        Ok(EckyConstraintsValidateResponse {
            thread_id: target_thread_id,
            message_id: target_message_id,
            title: design_output.title,
            version_name: design_output.version_name,
            resolved_from: TargetResolvedFrom::Base,
            source_digest: crate::mcp::macro_buffer::source_digest(&source),
            parameter_source,
            pass_count,
            fail_count,
            rows,
            authoring_lints,
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

pub async fn handle_ecky_ast_replace_and_render(
    state: &AppState,
    app: &dyn PathResolver,
    req: EckyAstReplaceAndRenderRequest,
    ctx: &AgentContext,
) -> AppResult<MacroReplaceResponse> {
    if !ecky_ast_authoring_enabled(state) {
        return Err(AppError::validation(
            "Ecky AST authoring is disabled. Set mcp.eckyAstAuthoring=true to expose ecky_ast_replace_and_render.",
        ));
    }

    let ctx = ctx.with_override(&req.identity);
    let preview = session_render_preview_for_request(
        &ctx,
        req.thread_id.as_deref(),
        req.message_id.as_deref(),
    );
    let (thread_id, message_id, design_output) = if let Some(preview) = preview {
        (preview.thread_id, preview.preview_id, preview.design_output)
    } else {
        let conn = state.db.lock().await;
        let target = crate::services::target::resolve_editable_target(
            &conn,
            app,
            req.thread_id.clone(),
            req.message_id.clone(),
        )?;
        drop(conn);
        (target.thread_id, target.message_id, target.design_output)
    };

    if design_output.source_language != crate::models::SourceLanguage::EckyIrV0 {
        return Err(AppError::validation(format!(
            "ecky_ast_replace_and_render only supports sourceLanguage=ecky. Target sourceLanguage={}.",
            design_output.source_language.as_str()
        )));
    }

    let source = design_output.macro_code.clone();
    let program = crate::ecky_scheme::compile_to_core_program(&source)
        .map_err(|err| AppError::validation(format!("Failed to compile Ecky source: {err}")))?;
    let resolved_path = resolve_ecky_ast_patch_path(
        &source,
        &program,
        req.path.as_deref(),
        req.stable_node_key.as_deref(),
        "ecky_ast_replace_and_render",
    )?;

    let next_source = replace_ecky_ast_source(
        &source,
        &req.source_digest,
        &resolved_path,
        &req.expected_node_digest,
        &req.operation,
        req.replacement_source.as_deref(),
        req.new_name.as_deref(),
    )?;

    handle_macro_preview_render(
        state,
        app,
        MacroReplaceRequest {
            identity: req.identity,
            thread_id: Some(thread_id),
            message_id: Some(message_id),
            macro_code: next_source,
            macro_dialect: Some(MacroDialect::EckyIrV0),
            ui_spec: None,
            parameters: req.parameters,
            post_processing: req.post_processing,
            geometry_backend: req.geometry_backend,
        },
        &ctx,
    )
    .await
}

pub async fn handle_ecky_ast_patch_validate(
    state: &AppState,
    app: &dyn PathResolver,
    req: EckyAstPatchValidateRequest,
    _ctx: &AgentContext,
) -> AppResult<EckyAstPatchValidateResponse> {
    if !ecky_ast_authoring_enabled(state) {
        return Err(AppError::validation(
            "Ecky AST authoring is disabled. Set mcp.eckyAstAuthoring=true to expose ecky_ast_patch_validate.",
        ));
    }

    let conn = state.db.lock().await;
    let target = crate::services::target::resolve_editable_target(
        &conn,
        app,
        req.thread_id.clone(),
        req.message_id.clone(),
    )?;
    drop(conn);

    if target.design_output.source_language != crate::models::SourceLanguage::EckyIrV0 {
        return Err(AppError::validation(format!(
            "ecky_ast_patch_validate only supports sourceLanguage=ecky. Target sourceLanguage={}.",
            target.design_output.source_language.as_str()
        )));
    }

    let source = target.design_output.macro_code.clone();
    let source_program = crate::ecky_scheme::compile_to_core_program(&source)
        .map_err(|err| AppError::validation(format!("Failed to compile Ecky source: {err}")))?;
    let resolved_path = resolve_ecky_ast_patch_path(
        &source,
        &source_program,
        req.path.as_deref(),
        req.stable_node_key.as_deref(),
        "ecky_ast_patch_validate",
    )?;
    let (next_source, new_node_digest, new_path, diff) = validate_ecky_ast_patch(
        &source,
        &req.source_digest,
        &resolved_path,
        &req.expected_node_digest,
        &req.operation,
        req.replacement_source.as_deref(),
        req.new_name.as_deref(),
    )?;
    let next_program = crate::ecky_scheme::compile_to_core_program(&next_source)
        .map_err(|err| AppError::validation(format!("Failed to compile Ecky source: {err}")))?;
    let operation = match req.operation {
        EckyAstEditOperation::Replace => "replace",
        EckyAstEditOperation::InsertBefore => "insertBefore",
        EckyAstEditOperation::InsertAfter => "insertAfter",
        EckyAstEditOperation::Delete => "delete",
        EckyAstEditOperation::Rename => "rename",
    };
    let affected_path_details = vec![EckyAstPatchAffectedPath {
        change: operation.to_string(),
        old_path: resolved_path.clone(),
        new_path: new_path.clone(),
        old_digest: req.expected_node_digest.clone(),
        new_digest: new_node_digest.clone(),
    }];

    let authoring_context = crate::mcp::authoring::target_authoring_context(&target.design_output);
    let mut affected_paths = vec![resolved_path.clone()];
    if !new_path.is_empty() && new_path != resolved_path {
        affected_paths.push(new_path.clone());
    }
    let edited_path_for_summary = if new_path.is_empty() {
        resolved_path.clone()
    } else {
        new_path.clone()
    };
    let affected_node_keys = affected_node_keys_for_patch(
        &source,
        &source_program,
        &resolved_path,
        &next_source,
        &next_program,
        &new_path,
    );
    let dependency_impact = Some(dependency_impact_for_patch(
        &next_program,
        &edited_path_for_summary,
        &affected_paths,
    ));

    Ok(EckyAstPatchValidateResponse {
        thread_id: target.thread_id,
        message_id: target.message_id,
        title: target.design_output.title,
        version_name: target.design_output.version_name,
        resolved_from: map_target_resolved_from(target.resolved_from),
        operation: operation.to_string(),
        edited_path: edited_path_for_summary,
        status: "valid".to_string(),
        source_digest: req.source_digest,
        new_source_digest: crate::mcp::macro_buffer::source_digest(&next_source),
        old_node_digest: req.expected_node_digest,
        new_node_digest,
        affected_paths,
        affected_path_details,
        affected_node_keys,
        dependency_impact,
        diff,
        authoring_context,
    })
}
