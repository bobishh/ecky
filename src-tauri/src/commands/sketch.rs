use tauri::AppHandle;

use crate::models::{
    AppResult, ArtifactBundle, PathResolver, SketchBrepCandidateRequest,
    SketchBrepCandidateResponse, SketchDraftRequest, SketchDraftSource, SketchPreviewHullRequest,
    SketchSuggestionRequest, SketchSuggestionResponse,
};
use crate::sketch_draft_runtime;

#[tauri::command]
#[specta::specta]
pub async fn generate_sketch_draft_source(
    request: SketchDraftRequest,
) -> AppResult<SketchDraftSource> {
    sketch_draft_runtime::generate_sketch_draft_source(request)
}

#[tauri::command]
#[specta::specta]
pub async fn suggest_sketch_features(
    request: SketchSuggestionRequest,
) -> AppResult<SketchSuggestionResponse> {
    Ok(sketch_draft_runtime::suggest_sketch_features(request))
}

pub async fn generate_sketch_draft_preview_for_app(
    app: &dyn PathResolver,
    request: SketchDraftRequest,
) -> AppResult<(SketchDraftSource, ArtifactBundle)> {
    sketch_draft_runtime::generate_sketch_draft_preview(request, app)
}

#[tauri::command]
#[specta::specta]
pub async fn generate_sketch_draft_preview(
    request: SketchDraftRequest,
    app: AppHandle,
) -> AppResult<(SketchDraftSource, ArtifactBundle)> {
    generate_sketch_draft_preview_for_app(&app, request).await
}

pub async fn generate_sketch_preview_hull_for_app(
    app: &dyn PathResolver,
    request: SketchPreviewHullRequest,
) -> AppResult<(SketchDraftSource, ArtifactBundle)> {
    sketch_draft_runtime::generate_sketch_preview_hull(request, app)
}

#[tauri::command]
#[specta::specta]
pub async fn generate_sketch_preview_hull(
    request: SketchPreviewHullRequest,
    app: AppHandle,
) -> AppResult<(SketchDraftSource, ArtifactBundle)> {
    generate_sketch_preview_hull_for_app(&app, request).await
}

#[tauri::command]
#[specta::specta]
pub async fn analyze_sketch_brep_candidates(
    request: SketchBrepCandidateRequest,
) -> AppResult<SketchBrepCandidateResponse> {
    sketch_draft_runtime::analyze_sketch_brep_candidates(request)
}
