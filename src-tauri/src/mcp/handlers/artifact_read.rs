use super::{artifact_bundle_digest, persist_agent_session, AgentContext};
use crate::mcp::contracts::{
    ArtifactFeatureGraphGetRequest, ArtifactFeatureGraphGetResponse, ArtifactManifestRequest,
    ArtifactManifestResponse,
};
use crate::models::{AppError, AppResult, AppState, PathResolver};

pub async fn handle_artifact_manifest_get(
    state: &AppState,
    app: &dyn PathResolver,
    req: ArtifactManifestRequest,
    ctx: &AgentContext,
) -> AppResult<ArtifactManifestResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let conn = state.db.lock().await;
    let target = crate::services::target::resolve_editable_target(
        &conn,
        app,
        req.thread_id.clone(),
        req.message_id.clone(),
    )?;
    drop(conn);

    let requested_model_id = req
        .model_id
        .clone()
        .or_else(|| target.model_id())
        .ok_or_else(|| AppError::validation("Target has no artifact modelId."))?;

    {
        let conn = state.db.lock().await;
        persist_agent_session(
            &conn,
            ctx,
            Some(target.thread_id.clone()),
            Some(target.message_id.clone()),
            Some(requested_model_id.clone()),
            "reading",
            "Reading runtime artifact manifest.",
        )?;
    }

    let (artifact_bundle, model_manifest) = match (
        target.artifact_bundle.clone(),
        target.model_manifest.clone(),
    ) {
        (Some(bundle), Some(manifest)) if bundle.model_id == requested_model_id => {
            (bundle, manifest)
        }
        _ => {
            let bundle = crate::model_runtime::read_artifact_bundle(app, &requested_model_id)?;
            let manifest = crate::model_runtime::read_model_manifest(app, &requested_model_id)?;
            (bundle, manifest)
        }
    };

    crate::models::validate_model_runtime_bundle(&model_manifest, &artifact_bundle)?;
    let digest = artifact_bundle_digest(&artifact_bundle);

    Ok(ArtifactManifestResponse {
        thread_id: target.thread_id,
        message_id: target.message_id,
        model_id: requested_model_id,
        digest,
        artifact_bundle,
        model_manifest,
        runtime_manifest_valid: true,
    })
}

pub async fn handle_artifact_feature_graph_get(
    state: &AppState,
    app: &dyn PathResolver,
    req: ArtifactFeatureGraphGetRequest,
    ctx: &AgentContext,
) -> AppResult<ArtifactFeatureGraphGetResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let conn = state.db.lock().await;
    let target = crate::services::target::resolve_editable_target(
        &conn,
        app,
        req.thread_id.clone(),
        req.message_id.clone(),
    )?;
    drop(conn);

    let requested_model_id = req
        .model_id
        .clone()
        .or_else(|| target.model_id())
        .ok_or_else(|| AppError::validation("Target has no artifact modelId."))?;

    {
        let conn = state.db.lock().await;
        persist_agent_session(
            &conn,
            ctx,
            Some(target.thread_id.clone()),
            Some(target.message_id.clone()),
            Some(requested_model_id.clone()),
            "reading",
            "Reading artifact feature graph.",
        )?;
    }

    let (artifact_bundle, model_manifest) =
        crate::model_runtime::read_runtime_bundle(app, &requested_model_id).map_err(|err| {
            if err.message.contains("Failed to read model manifest") {
                AppError::validation(format!(
                    "No model manifest found for modelId '{}'. artifact_feature_graph_get requires a runtime manifest.",
                    requested_model_id
                ))
            } else {
                err
            }
        })?;
    if artifact_bundle.model_id != requested_model_id
        || model_manifest.model_id != requested_model_id
    {
        return Err(AppError::validation(format!(
            "Runtime manifest modelId does not match requested modelId '{}'.",
            requested_model_id
        )));
    }
    crate::models::validate_model_runtime_bundle(&model_manifest, &artifact_bundle)?;

    Ok(ArtifactFeatureGraphGetResponse {
        thread_id: target.thread_id,
        message_id: target.message_id,
        model_id: requested_model_id,
        artifact_digest: artifact_bundle_digest(&artifact_bundle),
        feature_graph: model_manifest.feature_graph,
        correspondence_graph: model_manifest.correspondence_graph,
    })
}
