use super::{
    artifact_bundle_digest, handle_macro_preview_render, persist_agent_session,
    session_render_preview_for_request, try_record_agent_error, AgentContext,
};
use crate::mcp::contracts::{
    AgentIdentityOverride, MacroBufferApplyPatchRequest, MacroBufferEditResponse,
    MacroBufferGetRequest, MacroBufferGetResponse, MacroBufferLine, MacroBufferRenderRequest,
    MacroBufferReplaceAndRenderRequest, MacroBufferReplaceAndRenderResponse,
    MacroBufferReplacement, MacroReplaceRequest, MacroReplaceResponse, TargetResolvedFrom,
};
use crate::models::{AppError, AppResult, AppState, MacroDialect, PathResolver};
use std::collections::HashMap;
use std::sync::{Mutex as StdMutex, OnceLock};

#[derive(Debug, Clone)]
struct SessionMacroBuffer {
    thread_id: String,
    message_id: String,
    macro_code: String,
    macro_dialect: MacroDialect,
    post_processing: Option<crate::models::PostProcessingSpec>,
    geometry_backend: crate::models::GeometryBackend,
}

static MACRO_BUFFERS: OnceLock<StdMutex<HashMap<String, SessionMacroBuffer>>> = OnceLock::new();

fn macro_buffers() -> &'static StdMutex<HashMap<String, SessionMacroBuffer>> {
    MACRO_BUFFERS.get_or_init(|| StdMutex::new(HashMap::new()))
}

pub(super) fn macro_buffer_digest(macro_code: &str) -> String {
    crate::mcp::macro_buffer::source_digest(macro_code)
}

const DEFAULT_MACRO_BUFFER_WINDOW_LINES: usize = 200;

pub(super) fn macro_buffer_lines(macro_code: &str) -> Vec<MacroBufferLine> {
    macro_code
        .lines()
        .enumerate()
        .map(|(idx, text)| MacroBufferLine {
            line_number: idx + 1,
            text: text.to_string(),
        })
        .collect()
}

pub(super) fn macro_buffer_line_window(
    lines: &[MacroBufferLine],
    start_line: Option<usize>,
    end_line: Option<usize>,
) -> AppResult<(usize, usize, bool, Vec<MacroBufferLine>)> {
    let line_count = lines.len();
    if line_count == 0 {
        return Ok((0, 0, false, Vec::new()));
    }

    let start = start_line.unwrap_or(1);
    if start == 0 || start > line_count {
        return Err(AppError::validation(format!(
            "Macro buffer startLine {} is outside buffer line count {}.",
            start, line_count
        )));
    }

    let requested_end = end_line.unwrap_or_else(|| {
        std::cmp::min(
            line_count,
            start.saturating_add(DEFAULT_MACRO_BUFFER_WINDOW_LINES - 1),
        )
    });
    if requested_end < start {
        return Err(AppError::validation(format!(
            "Macro buffer endLine {} is before startLine {}.",
            requested_end, start
        )));
    }

    let end = std::cmp::min(requested_end, line_count);
    let window = lines[(start - 1)..end].to_vec();
    Ok((start, end, start > 1 || end < line_count, window))
}

pub(super) fn apply_macro_buffer_replacements(
    macro_code: &str,
    expected_digest: &str,
    replacements: &[MacroBufferReplacement],
) -> AppResult<String> {
    crate::mcp::macro_buffer::assert_expected_digest(macro_code, expected_digest).map_err(
        |err| {
            AppError::validation(format!(
                "Macro {} Read macro_buffer_get again before patching.",
                err.message.replacen("Buffer", "buffer", 1)
            ))
        },
    )?;

    if replacements.is_empty() {
        return Err(AppError::validation(
            "macro_buffer_replace_and_preview requires at least one replacement.",
        ));
    }

    let had_trailing_newline = macro_code.ends_with('\n');
    let mut lines: Vec<String> = macro_code.lines().map(str::to_string).collect();
    let line_count = lines.len();
    let mut sorted = replacements.to_vec();
    sorted.sort_by_key(|replacement| replacement.start_line);

    let mut previous_end = 0usize;
    for replacement in &sorted {
        if replacement.start_line == 0 {
            return Err(AppError::validation(
                "Macro buffer replacement startLine is 1-based and must be >= 1.",
            ));
        }
        if replacement.end_line < replacement.start_line {
            return Err(AppError::validation(format!(
                "Macro buffer replacement has endLine {} before startLine {}.",
                replacement.end_line, replacement.start_line
            )));
        }
        if replacement.end_line > line_count {
            return Err(AppError::validation(format!(
                "Macro buffer replacement line range {}..{} exceeds line count {}.",
                replacement.start_line, replacement.end_line, line_count
            )));
        }
        if replacement.start_line <= previous_end {
            return Err(AppError::validation(
                "Macro buffer replacements must not overlap.",
            ));
        }
        previous_end = replacement.end_line;
    }

    for replacement in sorted.iter().rev() {
        let start_idx = replacement.start_line - 1;
        let end_idx = replacement.end_line;
        let replacement_lines: Vec<String> =
            replacement.new_text.lines().map(str::to_string).collect();
        lines.splice(start_idx..end_idx, replacement_lines);
    }

    let mut patched = lines.join("\n");
    if had_trailing_newline {
        patched.push('\n');
    }
    Ok(patched)
}

fn get_session_macro_buffer(ctx: &AgentContext) -> AppResult<SessionMacroBuffer> {
    macro_buffers()
        .lock()
        .unwrap()
        .get(&ctx.session_id)
        .cloned()
        .ok_or_else(|| {
            AppError::validation(
                "No macro buffer for this session. Call macro_buffer_get before editing.",
            )
        })
}

fn set_session_macro_buffer(ctx: &AgentContext, buffer: SessionMacroBuffer) {
    macro_buffers()
        .lock()
        .unwrap()
        .insert(ctx.session_id.clone(), buffer);
}

pub async fn handle_macro_buffer_get(
    state: &AppState,
    app: &dyn PathResolver,
    req: MacroBufferGetRequest,
    ctx: &AgentContext,
) -> AppResult<MacroBufferGetResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let conn = state.db.lock().await;

    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = (|| -> AppResult<MacroBufferGetResponse> {
        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            None,
            "reading",
            "Reading macro buffer.",
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
        let macro_code = design_output.macro_code.clone();
        let lines = macro_buffer_lines(&macro_code);
        let line_count = lines.len();
        let digest = macro_buffer_digest(&macro_code);
        let (window_start_line, window_end_line, truncated, window_lines) =
            macro_buffer_line_window(&lines, req.start_line, req.end_line)?;
        set_session_macro_buffer(
            ctx,
            SessionMacroBuffer {
                thread_id: target_thread_id.clone(),
                message_id: target_message_id.clone(),
                macro_code: macro_code.clone(),
                macro_dialect: design_output.macro_dialect.clone(),
                post_processing: design_output.post_processing.clone(),
                geometry_backend: design_output.geometry_backend,
            },
        );

        Ok(MacroBufferGetResponse {
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
            source_language: design_output.source_language.as_str().to_string(),
            macro_dialect: design_output.macro_dialect,
            geometry_backend: design_output.geometry_backend.as_str().to_string(),
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

pub async fn handle_macro_buffer_replace_and_preview(
    state: &AppState,
    app: &dyn PathResolver,
    req: MacroBufferReplaceAndRenderRequest,
    ctx: &AgentContext,
) -> AppResult<MacroBufferReplaceAndRenderResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let mut buffer = get_session_macro_buffer(ctx)?;
    if let Some(thread_id) = &req.thread_id {
        if thread_id != &buffer.thread_id {
            return Err(AppError::validation(
                "macro_buffer_replace_and_preview threadId does not match session buffer.",
            ));
        }
    }
    if let Some(message_id) = &req.message_id {
        if message_id != &buffer.message_id {
            return Err(AppError::validation(
                "macro_buffer_replace_and_preview messageId does not match session buffer.",
            ));
        }
    }
    let patched_macro_code = apply_macro_buffer_replacements(
        &buffer.macro_code,
        &req.expected_digest,
        &req.replacements,
    )?;
    buffer.macro_code = patched_macro_code.clone();
    set_session_macro_buffer(ctx, buffer.clone());

    let render_response = handle_macro_preview_render(
        state,
        app,
        MacroReplaceRequest {
            identity: req.identity,
            thread_id: Some(buffer.thread_id.clone()),
            message_id: Some(buffer.message_id.clone()),
            macro_code: patched_macro_code,
            macro_dialect: Some(buffer.macro_dialect.clone()),
            ui_spec: req.ui_spec,
            parameters: req.parameters,
            post_processing: req.post_processing.or(buffer.post_processing.clone()),
            geometry_backend: Some(buffer.geometry_backend),
        },
        ctx,
    )
    .await?;

    let digest = macro_buffer_digest(&render_response.macro_code);
    let line_count = macro_buffer_lines(&render_response.macro_code).len();
    buffer.thread_id = render_response.thread_id.clone();
    buffer.message_id = render_response.message_id.clone();
    buffer.macro_code = render_response.macro_code.clone();
    set_session_macro_buffer(ctx, buffer);
    Ok(MacroBufferReplaceAndRenderResponse {
        thread_id: render_response.thread_id,
        message_id: render_response.message_id,
        digest,
        line_count,
        macro_code: render_response.macro_code,
        ui_spec: render_response.ui_spec,
        initial_params: render_response.initial_params,
        artifact_bundle: render_response.artifact_bundle,
        model_manifest: render_response.model_manifest,
        structural_verification: render_response.structural_verification,
        artifact_digest: render_response.artifact_digest,
    })
}

pub async fn handle_macro_buffer_replace_range(
    req: MacroBufferReplaceAndRenderRequest,
    ctx: &AgentContext,
) -> AppResult<MacroBufferEditResponse> {
    let ctx = ctx.with_override(&req.identity);
    let mut buffer = get_session_macro_buffer(&ctx)?;
    let window_start = req
        .replacements
        .iter()
        .map(|replacement| replacement.start_line)
        .min();
    buffer.macro_code = apply_macro_buffer_replacements(
        &buffer.macro_code,
        &req.expected_digest,
        &req.replacements,
    )?;
    let lines = macro_buffer_lines(&buffer.macro_code);
    let (window_start_line, window_end_line, truncated, window_lines) =
        macro_buffer_line_window(&lines, window_start, None)?;
    let response = MacroBufferEditResponse {
        digest: macro_buffer_digest(&buffer.macro_code),
        line_count: lines.len(),
        window_start_line,
        window_end_line,
        truncated,
        lines: window_lines,
    };
    set_session_macro_buffer(&ctx, buffer);
    Ok(response)
}

pub async fn handle_macro_buffer_apply_patch(
    req: MacroBufferApplyPatchRequest,
    ctx: &AgentContext,
) -> AppResult<MacroBufferEditResponse> {
    let ctx = ctx.with_override(&req.identity);
    let mut buffer = get_session_macro_buffer(&ctx)?;
    crate::mcp::macro_buffer::assert_expected_digest(&buffer.macro_code, &req.expected_digest)?;
    buffer.macro_code =
        crate::mcp::macro_buffer::apply_unified_patch(&buffer.macro_code, &req.patch)?;
    let lines = macro_buffer_lines(&buffer.macro_code);
    let (window_start_line, window_end_line, truncated, window_lines) =
        macro_buffer_line_window(&lines, None, None)?;
    let response = MacroBufferEditResponse {
        digest: macro_buffer_digest(&buffer.macro_code),
        line_count: lines.len(),
        window_start_line,
        window_end_line,
        truncated,
        lines: window_lines,
    };
    set_session_macro_buffer(&ctx, buffer);
    Ok(response)
}

pub async fn handle_macro_buffer_preview_render(
    state: &AppState,
    app: &dyn PathResolver,
    req: MacroBufferRenderRequest,
    ctx: &AgentContext,
) -> AppResult<MacroReplaceResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let mut buffer = get_session_macro_buffer(ctx)?;
    crate::mcp::macro_buffer::assert_expected_digest(&buffer.macro_code, &req.expected_digest)?;
    let response = handle_macro_preview_render(
        state,
        app,
        MacroReplaceRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some(buffer.thread_id.clone()),
            message_id: Some(buffer.message_id.clone()),
            macro_code: buffer.macro_code.clone(),
            macro_dialect: Some(buffer.macro_dialect.clone()),
            ui_spec: req.ui_spec,
            parameters: req.parameters,
            post_processing: req.post_processing.or(buffer.post_processing.clone()),
            geometry_backend: Some(buffer.geometry_backend),
        },
        ctx,
    )
    .await?;
    buffer.thread_id = response.thread_id.clone();
    buffer.message_id = response.message_id.clone();
    buffer.macro_code = response.macro_code.clone();
    set_session_macro_buffer(ctx, buffer);
    Ok(response)
}
