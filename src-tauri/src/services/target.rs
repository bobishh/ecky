use crate::db;
use crate::models::{
    AgentDraft, AppError, AppResult, ArtifactBundle, DesignOutput, LastDesignSnapshot,
    ModelManifest, PathResolver,
};
use crate::services::session::last_snapshot_path;
use std::fs;

pub struct ResolvedTarget {
    pub thread_id: String,
    pub message_id: String,
    pub design: Option<DesignOutput>,
    pub artifact_bundle: Option<ArtifactBundle>,
    pub model_manifest: Option<ModelManifest>,
    pub latest_draft: Option<AgentDraft>,
}

pub fn resolve_target(
    conn: &rusqlite::Connection,
    app: &dyn PathResolver,
    thread_id: Option<String>,
    message_id: Option<String>,
) -> AppResult<ResolvedTarget> {
    if let Some(msg_id) = message_id {
        let (output, tid) = db::get_message_output_and_thread(conn, &msg_id)
            .map_err(|e| AppError::persistence(e.to_string()))?
            .ok_or_else(|| AppError::not_found(format!("Message {} not found.", msg_id)))?;

        let (artifact_bundle, model_manifest, _) =
            db::get_message_runtime_and_thread(conn, &msg_id)
                .map_err(|e| AppError::persistence(e.to_string()))?
                .unwrap_or((None, None, tid.clone()));

        let latest_draft = db::get_agent_draft(conn, &tid, &msg_id)
            .map_err(|e| AppError::persistence(e.to_string()))?;

        return Ok(ResolvedTarget {
            thread_id: tid,
            message_id: msg_id,
            design: Some(output),
            artifact_bundle,
            model_manifest,
            latest_draft,
        });
    }

    if let Some(tid) = thread_id {
        let message_id = db::get_latest_successful_message_id_in_thread(conn, &tid)
            .map_err(|e| AppError::persistence(e.to_string()))?
            .ok_or_else(|| {
                AppError::validation(format!("Thread {} has no successful versions.", tid))
            })?;

        let target = resolve_target(conn, app, Some(tid.clone()), Some(message_id.clone()))?;
        let latest_draft = db::get_agent_draft(conn, &tid, &message_id)
            .map_err(|e| AppError::persistence(e.to_string()))?;

        return Ok(ResolvedTarget {
            thread_id: target.thread_id,
            message_id: target.message_id,
            design: target.design,
            artifact_bundle: target.artifact_bundle,
            model_manifest: target.model_manifest,
            latest_draft,
        });
    }

    // Try last_design.json
    let path = last_snapshot_path(app);
    if path.exists() {
        if let Ok(data) = fs::read_to_string(&path) {
            if let Ok(snapshot) = serde_json::from_str::<LastDesignSnapshot>(&data) {
                if let (Some(tid), Some(msg_id)) = (snapshot.thread_id, snapshot.message_id) {
                    return resolve_target(conn, app, Some(tid), Some(msg_id));
                }
            }
        }
    }

    Err(AppError::validation("No active target available."))
}
