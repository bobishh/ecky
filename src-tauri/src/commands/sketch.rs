use tauri::{AppHandle, State};

use crate::models::{
    AppError, AppResult, AppState, ArtifactBundle, BrepHiddenLineProjectionRequest,
    ComponentPackage, DesignParams, GeometryBackend, MacroDialect, PathResolver,
    SketchAcceptedBrepComponentPackageRequest, SketchBrepCandidateAcceptRequest,
    SketchBrepCandidateAcceptResponse, SketchBrepCandidateRequest, SketchBrepCandidateResponse,
    SketchDraftRequest, SketchDraftSource, SketchPreviewHullRequest, SketchSuggestionRequest,
    SketchSuggestionResponse, SketchView,
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

#[tauri::command]
#[specta::specta]
pub async fn accepted_brep_candidate_to_component_package(
    request: SketchAcceptedBrepComponentPackageRequest,
) -> AppResult<ComponentPackage> {
    sketch_draft_runtime::accepted_brep_candidate_to_component_package(request)
}

#[tauri::command]
#[specta::specta]
pub async fn accept_sketch_brep_candidate_solution(
    request: SketchBrepCandidateAcceptRequest,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<SketchBrepCandidateAcceptResponse> {
    let accepted = sketch_draft_runtime::generate_accepted_brep_candidate_source(request.clone())?;
    let mut bundle = crate::services::render::render_model(
        &accepted.draft_source.source,
        &DesignParams::new(),
        Some(MacroDialect::EckyIrV0),
        Some(GeometryBackend::EckyRust),
        None,
        &state,
        &app,
    )
    .await?;
    let step_artifact = sketch_draft_runtime::require_step_export_artifact(&bundle)?.clone();
    let hidden_line_response = crate::freecad::extract_brep_hidden_line_projections(
        &app,
        crate::services::render::configured_freecad_cmd(&state).as_deref(),
        BrepHiddenLineProjectionRequest {
            artifact_bundle: bundle.clone(),
            views: vec![SketchView::Front, SketchView::Top, SketchView::Side],
            tolerance: request.tolerance,
            sketch_document: Some(request.document),
        },
    )?;
    let validation = hidden_line_response.validation.as_ref().ok_or_else(|| {
        AppError::validation("Accepted BRep candidate hidden-line validation was not returned.")
    })?;
    if !validation.passed {
        let detail = validation
            .issues
            .first()
            .map(|issue| issue.message.as_str())
            .unwrap_or("hidden-line validation failed");
        return Err(AppError::validation(format!(
            "Accepted BRep candidate hidden-line validation failed: {}",
            detail
        )));
    }

    let mut evidence = accepted.evidence.clone();
    evidence.push(format!("STEP export artifact: {}", step_artifact.path));
    evidence.extend(validation.evidence.clone());
    bundle.export_artifacts.sort_by(|left, right| {
        left.format
            .cmp(&right.format)
            .then_with(|| left.path.cmp(&right.path))
    });

    Ok(SketchBrepCandidateAcceptResponse {
        draft_source: accepted.draft_source,
        artifact_bundle: bundle,
        hidden_line_response,
        candidate_response: accepted.candidate_response,
        accepted_solution: accepted.accepted_solution,
        evidence,
    })
}
