use tauri::AppHandle;

use crate::contracts::{AppError, AppResult};
use crate::services::drawing_import::{
    prepare_drawing_image as prepare_drawing_image_service,
    recognize_drawing_profile as recognize_drawing_profile_service,
    review_drawing_profile as review_drawing_profile_service, DrawingProfileReviewResponse,
    DrawingRecognitionResponse, PrepareDrawingImageRequest, PreparedDrawingImage,
    RecognizeDrawingProfileRequest, ReviewDrawingProfileRequest,
};

#[tauri::command]
#[specta::specta]
pub async fn prepare_drawing_image(
    request: PrepareDrawingImageRequest,
    app: AppHandle,
) -> AppResult<PreparedDrawingImage> {
    tauri::async_runtime::spawn_blocking(move || prepare_drawing_image_service(request, &app))
        .await
        .map_err(|error| {
            AppError::internal(format!("Drawing image preparation worker failed: {error}"))
        })?
}

#[tauri::command]
#[specta::specta]
pub async fn recognize_drawing_profile(
    request: RecognizeDrawingProfileRequest,
    app: AppHandle,
) -> AppResult<DrawingRecognitionResponse> {
    tauri::async_runtime::spawn_blocking(move || recognize_drawing_profile_service(request, &app))
        .await
        .map_err(|error| {
            AppError::internal(format!("Drawing recognition worker failed: {error}"))
        })?
}

#[tauri::command]
#[specta::specta]
pub async fn review_drawing_profile(
    request: ReviewDrawingProfileRequest,
    app: AppHandle,
) -> AppResult<DrawingProfileReviewResponse> {
    tauri::async_runtime::spawn_blocking(move || review_drawing_profile_service(request, &app))
        .await
        .map_err(|error| {
            AppError::internal(format!("Drawing profile review worker failed: {error}"))
        })?
}
