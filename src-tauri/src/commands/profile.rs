use tauri::{AppHandle, State};

use crate::contracts::{AppError, AppResult};
use crate::models::AppState;
use crate::services::profile_authoring::{
    apply_profile_feature as apply_profile_feature_service,
    derive_profile_geometry as derive_profile_geometry_service, ProfileFeatureApplyRequest,
    ProfileFeatureApplyResponse, ProfileGeometryRequest, ProfileGeometryResponse,
};

#[tauri::command]
#[specta::specta]
pub async fn derive_profile_geometry(
    request: ProfileGeometryRequest,
    app: AppHandle,
) -> AppResult<ProfileGeometryResponse> {
    tauri::async_runtime::spawn_blocking(move || derive_profile_geometry_service(request, &app))
        .await
        .map_err(|error| AppError::internal(format!("Profile geometry worker failed: {error}")))?
}

#[tauri::command]
#[specta::specta]
pub async fn apply_profile_feature(
    request: ProfileFeatureApplyRequest,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<ProfileFeatureApplyResponse> {
    apply_profile_feature_service(request, &state, &app).await
}
