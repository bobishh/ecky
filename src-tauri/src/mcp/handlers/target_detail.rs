use super::{
    agent_draft_from_session_render_preview, artifact_bundle_digest, build_shape_graph_packet,
    map_target_resolved_from, persist_agent_session, session_render_preview_for_request,
    try_record_agent_error, AgentContext,
};
use crate::db;
use crate::mcp::contracts::{TargetDetailRequest, TargetDetailResponse, TargetDetailSection};
use crate::models::{AppError, AppResult, AppState, PathResolver};

pub async fn handle_target_detail_get(
    state: &AppState,
    app: &dyn PathResolver,
    req: TargetDetailRequest,
    ctx: &AgentContext,
) -> AppResult<TargetDetailResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let conn = state.db.lock().await;

    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = (|| -> AppResult<TargetDetailResponse> {
        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            None,
            "reading",
            format!("Reading target detail section {:?}.", req.section),
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

        let (
            ui_spec,
            initial_params,
            artifact_bundle,
            artifact_paths,
            viewer_assets,
            export_artifacts,
            latest_draft,
            shape_graph,
        ) = match req.section {
            TargetDetailSection::UiSpec => (
                Some(target.design_output.ui_spec.clone()),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            ),
            TargetDetailSection::InitialParams => (
                None,
                Some(target.design_output.initial_params.clone()),
                None,
                None,
                None,
                None,
                None,
                None,
            ),
            TargetDetailSection::ArtifactBundle => {
                let digest = target.artifact_bundle.as_ref().map(artifact_bundle_digest);
                (None, None, Some(digest), None, None, None, None, None)
            }
            TargetDetailSection::ArtifactPaths => {
                let paths = target.artifact_bundle.as_ref().map(|b| {
                    let mut p: Vec<String> = vec![b.fcstd_path.clone()];
                    if let Some(mp) = &b.macro_path {
                        p.insert(0, mp.clone());
                    }
                    if !b.preview_stl_path.is_empty() {
                        p.push(b.preview_stl_path.clone());
                    }
                    p
                });
                (None, None, None, paths, None, None, None, None)
            }
            TargetDetailSection::ViewerAssets => (
                None,
                None,
                None,
                None,
                target
                    .artifact_bundle
                    .as_ref()
                    .map(|b| b.viewer_assets.clone()),
                None,
                None,
                None,
            ),
            TargetDetailSection::ExportArtifacts => (
                None,
                None,
                None,
                None,
                None,
                target
                    .artifact_bundle
                    .as_ref()
                    .map(|b| b.export_artifacts.clone()),
                None,
                None,
            ),
            TargetDetailSection::LatestDraft => {
                let latest_draft = if let Some(preview) = session_render_preview_for_request(
                    ctx,
                    Some(target.thread_id.as_str()),
                    Some(target.message_id.as_str()),
                ) {
                    Some(Some(agent_draft_from_session_render_preview(preview)))
                } else {
                    let draft = db::get_agent_draft_for_session(&conn, &ctx.session_id)
                        .map_err(|e| AppError::persistence(e.to_string()))?
                        .filter(|draft| {
                            draft.thread_id == target.thread_id
                                && (draft.preview_id == target.message_id
                                    || draft.base_message_id.as_deref()
                                        == Some(target.message_id.as_str()))
                        });
                    Some(draft)
                };
                (None, None, None, None, None, None, latest_draft, None)
            }
            TargetDetailSection::ShapeGraph => (
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(build_shape_graph_packet(
                    &target.design_output,
                    target.model_manifest.as_ref(),
                    target.artifact_bundle.as_ref(),
                    req.shape_graph_filters.as_deref().unwrap_or(&[]),
                )?),
            ),
        };

        let authoring_context =
            crate::mcp::authoring::target_authoring_context(&target.design_output);

        Ok(TargetDetailResponse {
            thread_id: target.thread_id,
            message_id: target.message_id,
            title: target.design_output.title,
            version_name: target.design_output.version_name,
            resolved_from: map_target_resolved_from(target.resolved_from),
            section: req.section,
            authoring_context,
            ui_spec,
            initial_params,
            artifact_bundle,
            artifact_paths,
            viewer_assets,
            export_artifacts,
            latest_draft,
            shape_graph,
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
