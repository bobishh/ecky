use std::io::Cursor;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use image::{
    DynamicImage, GenericImageView, GrayImage, ImageDecoder, ImageFormat, ImageReader, Luma,
    RgbaImage,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;

use crate::contracts::{AppError, AppResult, SketchView};
use crate::models::PathResolver;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Type)]
#[serde(rename_all = "lowercase")]
pub enum DrawingImageFormat {
    Png,
    Jpeg,
    Webp,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct DrawingImageIdentity {
    pub artifact_path: String,
    pub digest: String,
    pub width_px: u32,
    pub height_px: u32,
    pub format: DrawingImageFormat,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PrepareDrawingImageRequest {
    pub source_path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PreparedDrawingImage {
    pub image: DrawingImageIdentity,
    pub preview_data_url: String,
    pub preview_width_px: u32,
    pub preview_height_px: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct DrawingRegion {
    pub left_px: f64,
    pub top_px: f64,
    pub width_px: f64,
    pub height_px: f64,
    pub rotation_degrees: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct DrawingRectification {
    pub corners_px: [[f64; 2]; 4],
    pub aspect_ratio: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct DrawingScaleCalibration {
    pub start_px: [f64; 2],
    pub end_px: [f64; 2],
    pub distance_mm: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct DrawingImportEvidence {
    pub image: DrawingImageIdentity,
    pub view: SketchView,
    pub region: DrawingRegion,
    pub rectification: Option<DrawingRectification>,
    pub calibration: DrawingScaleCalibration,
    pub tolerance_mm: f64,
    pub extractor_version: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DrawingPrimitive {
    #[specta(rename_all = "camelCase")]
    Line {
        id: String,
        start_mm: [f64; 2],
        end_mm: [f64; 2],
        residual_mm: f64,
    },
    #[specta(rename_all = "camelCase")]
    Circle {
        id: String,
        center_mm: [f64; 2],
        radius_mm: f64,
        residual_mm: f64,
    },
    #[specta(rename_all = "camelCase")]
    Polyline {
        id: String,
        points_mm: Vec<[f64; 2]>,
        closed: bool,
        residual_mm: f64,
    },
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Type)]
#[serde(rename_all = "lowercase")]
pub enum DrawingProfileRole {
    Outer,
    Hole,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct DrawingProfileCandidate {
    pub id: String,
    pub primitive_ids: Vec<String>,
    pub role: DrawingProfileRole,
    pub parent_id: Option<String>,
    pub closed: bool,
    pub residual_mm: f64,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecognizeDrawingProfileRequest {
    pub image: DrawingImageIdentity,
    pub view: SketchView,
    pub region: DrawingRegion,
    pub rectification: Option<DrawingRectification>,
    pub calibration: DrawingScaleCalibration,
    #[serde(default)]
    pub excluded_regions: Vec<DrawingRegion>,
    pub tolerance_mm: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DrawingRecognitionResponse {
    pub evidence_id: String,
    pub evidence: DrawingImportEvidence,
    pub primitives: Vec<DrawingPrimitive>,
    pub profiles: Vec<DrawingProfileCandidate>,
    pub diagnostics: Vec<String>,
    pub transformed_preview_data_url: String,
    pub preview_width_px: u32,
    pub preview_height_px: u32,
    /// Row-major homogeneous matrix mapping normalized source-image pixels to preview pixels.
    pub source_to_preview_transform: [f64; 9],
    pub millimeters_per_pixel: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DrawingProfileApplyInput {
    pub evidence_id: String,
    pub primitives: Vec<DrawingPrimitive>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReviewDrawingProfileRequest {
    pub evidence_id: String,
    pub primitives: Vec<DrawingPrimitive>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DrawingProfileReviewResponse {
    pub evidence_id: String,
    pub profiles: Vec<DrawingProfileCandidate>,
    pub diagnostics: Vec<String>,
}

pub const DRAWING_EXTRACTOR_VERSION: &str = "drawing-centerline-v1";
const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_IMAGE_PIXELS: u64 = 40_000_000;
const MAX_COMPONENT_POINTS: usize = 8_192;
const MAX_LINE_FIT_WORK: usize = 32_000_000;
const MAX_DRAWING_PRIMITIVES: usize = 256;
const MAX_REVIEW_POINTS: usize = 65_536;

pub fn prepare_drawing_image(
    request: PrepareDrawingImageRequest,
    app: &dyn PathResolver,
) -> AppResult<PreparedDrawingImage> {
    let source_path = Path::new(&request.source_path);
    if !source_path.is_file() {
        return Err(AppError::validation(
            "Choose an existing PNG, JPEG, or WebP image.",
        ));
    }
    let metadata = std::fs::metadata(source_path)
        .map_err(|error| AppError::validation(format!("Could not inspect image: {error}")))?;
    if metadata.len() > MAX_IMAGE_BYTES {
        return Err(AppError::validation(format!(
            "Image is larger than the {} MiB limit.",
            MAX_IMAGE_BYTES / (1024 * 1024)
        )));
    }
    let bytes = std::fs::read(source_path)
        .map_err(|error| AppError::validation(format!("Could not read image: {error}")))?;
    let (format, normalized) = decode_supported_image(&bytes)?;
    let (width_px, height_px) = normalized.dimensions();
    validate_image_dimensions(width_px, height_px)?;
    let digest = format!("sha256:{:x}", Sha256::digest(&bytes));
    let extension = match format {
        DrawingImageFormat::Png => "png",
        DrawingImageFormat::Jpeg => "jpg",
        DrawingImageFormat::Webp => "webp",
    };
    let image_dir = app.app_data_dir().join("drawing-import").join("images");
    std::fs::create_dir_all(&image_dir).map_err(|error| {
        AppError::persistence(format!("Could not create drawing storage: {error}"))
    })?;
    let managed_path = image_dir.join(format!(
        "{}.{}",
        digest.trim_start_matches("sha256:"),
        extension
    ));
    if managed_path.exists() {
        if digest_file(&managed_path)? != digest {
            return Err(AppError::conflict(
                "Managed drawing image path already contains different data.",
            ));
        }
    } else {
        let staged = image_dir.join(format!(".{}.{}.stage", uuid::Uuid::new_v4(), extension));
        std::fs::write(&staged, &bytes).map_err(|error| {
            AppError::persistence(format!("Could not store drawing image: {error}"))
        })?;
        match std::fs::rename(&staged, &managed_path) {
            Ok(()) => {}
            Err(error) if managed_path.exists() => {
                let _ = std::fs::remove_file(staged);
                if digest_file(&managed_path)? != digest {
                    return Err(AppError::conflict(
                        "Managed drawing image path already contains different data.",
                    ));
                }
                let _ = error;
            }
            Err(error) => {
                let _ = std::fs::remove_file(staged);
                return Err(AppError::persistence(format!(
                    "Could not store drawing image: {error}"
                )));
            }
        }
    }
    let preview = normalized.thumbnail(960, 720);
    let (preview_width_px, preview_height_px) = preview.dimensions();
    Ok(PreparedDrawingImage {
        image: DrawingImageIdentity {
            artifact_path: managed_path.to_string_lossy().into_owned(),
            digest,
            width_px,
            height_px,
            format,
        },
        preview_data_url: image_data_url(preview)?,
        preview_width_px,
        preview_height_px,
    })
}

pub fn recognize_drawing_profile(
    request: RecognizeDrawingProfileRequest,
    app: &dyn PathResolver,
) -> AppResult<DrawingRecognitionResponse> {
    validate_recognition_request(&request)?;
    let managed_dir = app.app_data_dir().join("drawing-import");
    let image_path = Path::new(&request.image.artifact_path);
    if !image_path.starts_with(managed_dir.join("images")) {
        return Err(AppError::validation(
            "Prepare the drawing image in this workspace before recognizing it.",
        ));
    }
    let bytes = std::fs::read(image_path).map_err(|error| {
        AppError::validation(format!("Managed drawing image is unavailable: {error}"))
    })?;
    let actual_digest = format!("sha256:{:x}", Sha256::digest(&bytes));
    if actual_digest != request.image.digest {
        return Err(AppError::conflict(
            "Drawing image changed after preparation. Prepare it again before recognition.",
        ));
    }
    let (_, decoded) = decode_supported_image(&bytes)?;
    let (width, height) = decoded.dimensions();
    if width != request.image.width_px || height != request.image.height_px {
        return Err(AppError::conflict(
            "Prepared image dimensions no longer match its identity.",
        ));
    }
    let transform = build_image_transform(width, height, &request)?;
    let (preview, gray, excluded) =
        transform_image(&decoded, &transform, &request.excluded_regions)?;
    let calibration = calibrated_scale(&request, &transform)?;
    if request.tolerance_mm / calibration < 0.5 {
        return Err(AppError::validation(format!(
            "Requested fitting tolerance is finer than this image's pixel resolution ({:.4} mm/pixel). Increase tolerance or use a higher-resolution image.",
            calibration
        )));
    }
    let binary = threshold_drawing(&gray, &excluded);
    let skeleton = zhang_suen_thin(binary, gray.width(), gray.height());
    let (primitives, mut diagnostics) = fit_primitives(
        &skeleton,
        gray.width(),
        gray.height(),
        calibration,
        request.tolerance_mm,
    );
    if primitives.is_empty() {
        return Err(AppError::validation(format!(
            "No reliable centerline geometry found in this view region. Check crop, contrast, and calibration. {}",
            diagnostics.join("; ")
        )));
    }
    let profiles = assemble_profile_candidates(&primitives);
    if profiles.is_empty() {
        diagnostics.push(
            "No closed profile candidate was found; open geometry remains unselected.".into(),
        );
    }
    let evidence = DrawingImportEvidence {
        image: request.image.clone(),
        view: request.view.clone(),
        region: request.region.clone(),
        rectification: request.rectification.clone(),
        calibration: request.calibration.clone(),
        tolerance_mm: request.tolerance_mm,
        extractor_version: DRAWING_EXTRACTOR_VERSION.to_string(),
    };
    let evidence_bytes = serde_json::to_vec(&evidence).map_err(|error| {
        AppError::persistence(format!("Could not serialize drawing evidence: {error}"))
    })?;
    let evidence_id = format!("sha256:{:x}", Sha256::digest(&evidence_bytes));
    let evidence_path = managed_dir.join("evidence").join(format!(
        "{}.json",
        evidence_id.trim_start_matches("sha256:")
    ));
    let evidence_parent = evidence_path.parent().ok_or_else(|| {
        AppError::persistence("Could not determine drawing evidence storage directory.")
    })?;
    std::fs::create_dir_all(evidence_parent).map_err(|error| {
        AppError::persistence(format!("Could not store drawing evidence: {error}"))
    })?;
    if evidence_path.exists() {
        let stored = std::fs::read(&evidence_path).map_err(|error| {
            AppError::persistence(format!("Could not read drawing evidence: {error}"))
        })?;
        if stored != evidence_bytes {
            return Err(AppError::conflict("Drawing evidence identity collision."));
        }
    } else {
        std::fs::write(&evidence_path, evidence_bytes).map_err(|error| {
            AppError::persistence(format!("Could not store drawing evidence: {error}"))
        })?;
    }
    Ok(DrawingRecognitionResponse {
        evidence_id,
        evidence,
        primitives,
        profiles,
        diagnostics,
        transformed_preview_data_url: image_data_url(DynamicImage::ImageRgba8(preview))?,
        preview_width_px: gray.width(),
        preview_height_px: gray.height(),
        source_to_preview_transform: transform.source_to_view,
        millimeters_per_pixel: calibration,
    })
}

pub fn review_drawing_profile(
    request: ReviewDrawingProfileRequest,
    app: &dyn PathResolver,
) -> AppResult<DrawingProfileReviewResponse> {
    let _evidence = load_verified_evidence(&app.app_data_dir(), &request.evidence_id)?;
    let reviewed = review_profile_primitives(&request.primitives);
    Ok(DrawingProfileReviewResponse {
        evidence_id: request.evidence_id,
        profiles: reviewed.profiles,
        diagnostics: reviewed.diagnostics,
    })
}

pub(crate) fn load_verified_evidence(
    app_data_dir: &Path,
    evidence_id: &str,
) -> AppResult<DrawingImportEvidence> {
    let evidence = load_evidence(app_data_dir, evidence_id)?;
    let image_root = app_data_dir.join("drawing-import").join("images");
    if !Path::new(&evidence.image.artifact_path).starts_with(&image_root) {
        return Err(AppError::validation(
            "Drawing evidence does not refer to a managed image. Recognize it again.",
        ));
    }
    if digest_file(Path::new(&evidence.image.artifact_path))? != evidence.image.digest {
        return Err(AppError::conflict(
            "The source drawing changed after recognition. Recognize it again before applying.",
        ));
    }
    Ok(evidence)
}

#[derive(Clone, Copy)]
struct ImageTransform {
    source_to_view: [f64; 9],
    view_to_source: [f64; 9],
    width_px: u32,
    height_px: u32,
}

fn decode_supported_image(bytes: &[u8]) -> AppResult<(DrawingImageFormat, DynamicImage)> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| {
            AppError::validation(format!("Image format could not be read: {error}"))
        })?;
    let format = match reader.format() {
        Some(ImageFormat::Png) => DrawingImageFormat::Png,
        Some(ImageFormat::Jpeg) => DrawingImageFormat::Jpeg,
        Some(ImageFormat::WebP) => DrawingImageFormat::Webp,
        _ => return Err(AppError::validation("Choose a PNG, JPEG, or WebP image.")),
    };
    let (encoded_width, encoded_height) = reader.into_dimensions().map_err(|error| {
        AppError::validation(format!("Image dimensions could not be read: {error}"))
    })?;
    validate_image_dimensions(encoded_width, encoded_height)?;
    let decoder = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| AppError::validation(format!("Image format could not be read: {error}")))?
        .into_decoder()
        .map_err(|error| AppError::validation(format!("Image decoder failed: {error}")))?;
    let mut decoder = decoder;
    let orientation = decoder.orientation().map_err(|error| {
        AppError::validation(format!("Image orientation could not be read: {error}"))
    })?;
    let mut image = DynamicImage::from_decoder(decoder)
        .map_err(|error| AppError::validation(format!("Image decode failed: {error}")))?;
    image.apply_orientation(orientation);
    let (width, height) = image.dimensions();
    validate_image_dimensions(width, height)?;
    Ok((format, image))
}

fn validate_image_dimensions(width: u32, height: u32) -> AppResult<()> {
    let pixels = u64::from(width) * u64::from(height);
    if width == 0 || height == 0 || pixels > MAX_IMAGE_PIXELS {
        return Err(AppError::validation(format!(
            "Image dimensions {width}x{height} exceed the 40-million-pixel limit."
        )));
    }
    Ok(())
}

fn digest_file(path: &Path) -> AppResult<String> {
    let bytes = std::fs::read(path)
        .map_err(|error| AppError::validation(format!("Could not read image: {error}")))?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn image_data_url(image: DynamicImage) -> AppResult<String> {
    let mut bytes = Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, ImageFormat::Png)
        .map_err(|error| {
            AppError::persistence(format!("Could not create image preview: {error}"))
        })?;
    Ok(format!(
        "data:image/png;base64,{}",
        BASE64.encode(bytes.into_inner())
    ))
}

fn validate_recognition_request(request: &RecognizeDrawingProfileRequest) -> AppResult<()> {
    let region = &request.region;
    if ![
        region.left_px,
        region.top_px,
        region.width_px,
        region.height_px,
        region.rotation_degrees,
    ]
    .iter()
    .all(|value| value.is_finite())
        || region.width_px < 16.0
        || region.height_px < 16.0
        || region.rotation_degrees.abs() > 360.0
    {
        return Err(AppError::validation(
            "Select a valid drawing region and rotation.",
        ));
    }
    let calibration = &request.calibration;
    if !calibration.distance_mm.is_finite() || calibration.distance_mm <= 0.0 {
        return Err(AppError::validation(
            "Known calibration distance must be a positive number of millimeters.",
        ));
    }
    if !calibration
        .start_px
        .iter()
        .chain(calibration.end_px.iter())
        .all(|value| value.is_finite())
    {
        return Err(AppError::validation(
            "Calibration points must be finite image coordinates.",
        ));
    }
    if !request.tolerance_mm.is_finite() || !(0.02..=5.0).contains(&request.tolerance_mm) {
        return Err(AppError::validation(
            "Fitting tolerance must be between 0.02 and 5 mm.",
        ));
    }
    if request.image.artifact_path.trim().is_empty() || request.image.digest.trim().is_empty() {
        return Err(AppError::validation("Prepare an image before recognition."));
    }
    if let Some(rectification) = &request.rectification {
        if !rectification.aspect_ratio.is_finite() || rectification.aspect_ratio <= 0.0 {
            return Err(AppError::validation(
                "Perspective correction needs a positive known rectangle aspect ratio.",
            ));
        }
        if region.rotation_degrees.abs() > 1.0e-6 {
            return Err(AppError::validation(
                "Set region rotation to zero when using four-corner perspective correction.",
            ));
        }
        validate_rectification_corners(rectification)?;
    }
    Ok(())
}

fn validate_rectification_corners(rectification: &DrawingRectification) -> AppResult<()> {
    let corners = rectification.corners_px;
    if corners.iter().flatten().any(|value| !value.is_finite()) {
        return Err(AppError::validation(
            "Perspective corners must be finite image coordinates.",
        ));
    }
    let crosses = (0..4)
        .map(|index| {
            let a = corners[index];
            let b = corners[(index + 1) % 4];
            let c = corners[(index + 2) % 4];
            (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0])
        })
        .collect::<Vec<_>>();
    if crosses.iter().any(|cross| cross.abs() < 1.0)
        || !(crosses.iter().all(|cross| *cross > 0.0) || crosses.iter().all(|cross| *cross < 0.0))
    {
        return Err(AppError::validation(
            "Perspective corners must form a non-crossed, non-collinear rectangle.",
        ));
    }
    let width = ((corners[1][0] - corners[0][0]).hypot(corners[1][1] - corners[0][1])
        + (corners[2][0] - corners[3][0]).hypot(corners[2][1] - corners[3][1]))
        / 2.0;
    let height = ((corners[3][0] - corners[0][0]).hypot(corners[3][1] - corners[0][1])
        + (corners[2][0] - corners[1][0]).hypot(corners[2][1] - corners[1][1]))
        / 2.0;
    if width < 16.0 || height < 16.0 || width / height > 20.0 || height / width > 20.0 {
        return Err(AppError::validation(
            "Perspective rectangle is too small or ill-conditioned.",
        ));
    }
    Ok(())
}

fn build_image_transform(
    image_width: u32,
    image_height: u32,
    request: &RecognizeDrawingProfileRequest,
) -> AppResult<ImageTransform> {
    let region = &request.region;
    let right = region.left_px + region.width_px;
    let bottom = region.top_px + region.height_px;
    if region.left_px < 0.0
        || region.top_px < 0.0
        || right > image_width as f64
        || bottom > image_height as f64
    {
        return Err(AppError::validation(
            "Drawing region must stay inside the prepared image.",
        ));
    }
    let (raw_width, raw_height, source_to_raw) = if let Some(rectification) = &request.rectification
    {
        let corners = rectification.corners_px;
        if corners.iter().any(|point| {
            point[0] < region.left_px
                || point[0] > right
                || point[1] < region.top_px
                || point[1] > bottom
        }) {
            return Err(AppError::validation(
                "All perspective corners must lie inside the selected drawing region.",
            ));
        }
        let width = ((corners[1][0] - corners[0][0]).hypot(corners[1][1] - corners[0][1])
            + (corners[2][0] - corners[3][0]).hypot(corners[2][1] - corners[3][1]))
            / 2.0;
        let height = width / rectification.aspect_ratio;
        let transform = homography(
            &corners,
            &[[0.0, 0.0], [width, 0.0], [width, height], [0.0, height]],
        )?;
        (width, height, transform)
    } else {
        let angle = region.rotation_degrees.to_radians();
        let cosine = angle.cos();
        let sine = angle.sin();
        let width = region.width_px * cosine.abs() + region.height_px * sine.abs();
        let height = region.width_px * sine.abs() + region.height_px * cosine.abs();
        let transform = [
            cosine,
            -sine,
            -cosine * region.left_px + sine * region.top_px - cosine * region.width_px / 2.0
                + sine * region.height_px / 2.0
                + width / 2.0,
            sine,
            cosine,
            -sine * region.left_px
                - cosine * region.top_px
                - sine * region.width_px / 2.0
                - cosine * region.height_px / 2.0
                + height / 2.0,
            0.0,
            0.0,
            1.0,
        ];
        (width, height, transform)
    };
    let pixel_count = raw_width * raw_height;
    let downscale = (1.0f64)
        .min(4096.0 / raw_width.max(raw_height))
        .min((8_000_000.0 / pixel_count).sqrt());
    let width = (raw_width * downscale).round().max(16.0) as u32;
    let height = (raw_height * downscale).round().max(16.0) as u32;
    let scale = width as f64 / raw_width;
    let source_to_view = [
        source_to_raw[0] * scale,
        source_to_raw[1] * scale,
        source_to_raw[2] * scale,
        source_to_raw[3] * scale,
        source_to_raw[4] * scale,
        source_to_raw[5] * scale,
        source_to_raw[6],
        source_to_raw[7],
        source_to_raw[8],
    ];
    let view_to_source = invert_matrix(source_to_view)
        .ok_or_else(|| AppError::validation("Drawing region transform is ill-conditioned."))?;
    Ok(ImageTransform {
        source_to_view,
        view_to_source,
        width_px: width,
        height_px: height,
    })
}

fn homography(source: &[[f64; 2]; 4], target: &[[f64; 2]; 4]) -> AppResult<[f64; 9]> {
    let mut matrix = [[0.0; 9]; 8];
    for index in 0..4 {
        let [x, y] = source[index];
        let [u, v] = target[index];
        matrix[index * 2] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
        matrix[index * 2 + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
    }
    for pivot in 0..8 {
        let best = (pivot..8)
            .max_by(|left, right| {
                matrix[*left][pivot]
                    .abs()
                    .total_cmp(&matrix[*right][pivot].abs())
            })
            .ok_or_else(|| {
                AppError::validation("Perspective corners do not define a stable plane.")
            })?;
        if matrix[best][pivot].abs() < 1.0e-10 {
            return Err(AppError::validation(
                "Perspective corners are ill-conditioned.",
            ));
        }
        matrix.swap(pivot, best);
        let divisor = matrix[pivot][pivot];
        for column in pivot..=8 {
            matrix[pivot][column] /= divisor;
        }
        for row in 0..8 {
            if row == pivot {
                continue;
            }
            let factor = matrix[row][pivot];
            for column in pivot..=8 {
                matrix[row][column] -= factor * matrix[pivot][column];
            }
        }
    }
    Ok([
        matrix[0][8],
        matrix[1][8],
        matrix[2][8],
        matrix[3][8],
        matrix[4][8],
        matrix[5][8],
        matrix[6][8],
        matrix[7][8],
        1.0,
    ])
}

fn invert_matrix(m: [f64; 9]) -> Option<[f64; 9]> {
    let determinant = m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6])
        + m[2] * (m[3] * m[7] - m[4] * m[6]);
    if determinant.abs() < 1.0e-12 {
        return None;
    }
    let inv = [
        m[4] * m[8] - m[5] * m[7],
        m[2] * m[7] - m[1] * m[8],
        m[1] * m[5] - m[2] * m[4],
        m[5] * m[6] - m[3] * m[8],
        m[0] * m[8] - m[2] * m[6],
        m[2] * m[3] - m[0] * m[5],
        m[3] * m[7] - m[4] * m[6],
        m[1] * m[6] - m[0] * m[7],
        m[0] * m[4] - m[1] * m[3],
    ];
    Some(inv.map(|value| value / determinant))
}

fn map_point(matrix: [f64; 9], point: [f64; 2]) -> Option<[f64; 2]> {
    let denominator = matrix[6] * point[0] + matrix[7] * point[1] + matrix[8];
    if denominator.abs() < 1.0e-12 {
        return None;
    }
    Some([
        (matrix[0] * point[0] + matrix[1] * point[1] + matrix[2]) / denominator,
        (matrix[3] * point[0] + matrix[4] * point[1] + matrix[5]) / denominator,
    ])
}

fn transform_image(
    source: &DynamicImage,
    transform: &ImageTransform,
    exclusions: &[DrawingRegion],
) -> AppResult<(RgbaImage, GrayImage, Vec<bool>)> {
    let source_rgba = source.to_rgba8();
    let source_gray = source.to_luma8();
    let mut preview = RgbaImage::new(transform.width_px, transform.height_px);
    let mut gray = GrayImage::from_pixel(transform.width_px, transform.height_px, Luma([255]));
    let mut excluded = vec![false; transform.width_px as usize * transform.height_px as usize];
    for y in 0..transform.height_px {
        for x in 0..transform.width_px {
            let Some([source_x, source_y]) =
                map_point(transform.view_to_source, [x as f64 + 0.5, y as f64 + 0.5])
            else {
                continue;
            };
            let source_x = source_x.round() as i64;
            let source_y = source_y.round() as i64;
            if source_x < 0
                || source_y < 0
                || source_x >= source.width() as i64
                || source_y >= source.height() as i64
            {
                continue;
            }
            let source_x = source_x as u32;
            let source_y = source_y as u32;
            preview.put_pixel(x, y, *source_rgba.get_pixel(source_x, source_y));
            gray.put_pixel(x, y, *source_gray.get_pixel(source_x, source_y));
            if exclusions.iter().any(|region| {
                source_x as f64 >= region.left_px
                    && source_x as f64 <= region.left_px + region.width_px
                    && source_y as f64 >= region.top_px
                    && source_y as f64 <= region.top_px + region.height_px
            }) {
                excluded[y as usize * transform.width_px as usize + x as usize] = true;
            }
        }
    }
    Ok((preview, gray, excluded))
}

fn calibrated_scale(
    request: &RecognizeDrawingProfileRequest,
    transform: &ImageTransform,
) -> AppResult<f64> {
    let start = map_point(transform.source_to_view, request.calibration.start_px)
        .ok_or_else(|| AppError::validation("Calibration line cannot be mapped into the view."))?;
    let end = map_point(transform.source_to_view, request.calibration.end_px)
        .ok_or_else(|| AppError::validation("Calibration line cannot be mapped into the view."))?;
    if [start, end]
        .iter()
        .flatten()
        .any(|value| !value.is_finite())
        || [start, end].iter().any(|point| {
            point[0] < -1.0e-3
                || point[1] < -1.0e-3
                || point[0] > transform.width_px as f64 + 1.0e-3
                || point[1] > transform.height_px as f64 + 1.0e-3
        })
    {
        return Err(AppError::validation(
            "Calibration points must lie inside the selected view region.",
        ));
    }
    let pixel_distance = (end[0] - start[0]).hypot(end[1] - start[1]);
    if pixel_distance < 4.0 {
        return Err(AppError::validation(
            "Choose calibration points at least four pixels apart.",
        ));
    }
    Ok(request.calibration.distance_mm / pixel_distance)
}

fn threshold_drawing(gray: &GrayImage, excluded: &[bool]) -> Vec<bool> {
    let mut histogram = [0u64; 256];
    for (index, pixel) in gray.pixels().enumerate() {
        if !excluded[index] {
            histogram[pixel[0] as usize] += 1;
        }
    }
    let threshold = otsu_threshold(&histogram);
    let mut foreground = gray
        .pixels()
        .map(|pixel| pixel[0] <= threshold)
        .collect::<Vec<_>>();
    remove_small_components(&mut foreground, gray.width(), gray.height(), 8);
    for (index, is_foreground) in foreground.iter_mut().enumerate() {
        *is_foreground &= !excluded[index];
    }
    foreground
}

fn otsu_threshold(histogram: &[u64; 256]) -> u8 {
    let total: u64 = histogram.iter().sum();
    let sum: f64 = histogram
        .iter()
        .enumerate()
        .map(|(value, count)| value as f64 * *count as f64)
        .sum();
    let mut background_weight = 0u64;
    let mut background_sum = 0.0;
    let mut best_variance = -1.0;
    let mut best = 128u8;
    for (value, count) in histogram.iter().enumerate() {
        background_weight += count;
        if background_weight == 0 {
            continue;
        }
        let foreground_weight = total.saturating_sub(background_weight);
        if foreground_weight == 0 {
            break;
        }
        background_sum += value as f64 * *count as f64;
        let background_mean = background_sum / background_weight as f64;
        let foreground_mean = (sum - background_sum) / foreground_weight as f64;
        let variance = background_weight as f64
            * foreground_weight as f64
            * (background_mean - foreground_mean).powi(2);
        if variance > best_variance {
            best_variance = variance;
            best = value as u8;
        }
    }
    best
}

fn remove_small_components(foreground: &mut [bool], width: u32, height: u32, minimum: usize) {
    let mut seen = vec![false; foreground.len()];
    let mut queue = Vec::new();
    for start in 0..foreground.len() {
        if !foreground[start] || seen[start] {
            continue;
        }
        seen[start] = true;
        queue.clear();
        queue.push(start);
        let mut cursor = 0;
        while cursor < queue.len() {
            let index = queue[cursor];
            cursor += 1;
            for next in pixel_neighbors(index, width, height) {
                if foreground[next] && !seen[next] {
                    seen[next] = true;
                    queue.push(next);
                }
            }
        }
        if queue.len() < minimum {
            for index in &queue {
                foreground[*index] = false;
            }
        }
    }
}

fn pixel_neighbors(index: usize, width: u32, height: u32) -> Vec<usize> {
    let x = index as i32 % width as i32;
    let y = index as i32 / width as i32;
    let mut neighbors = Vec::with_capacity(8);
    for dy in -1..=1 {
        for dx in -1..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let nx = x + dx;
            let ny = y + dy;
            if nx >= 0 && ny >= 0 && nx < width as i32 && ny < height as i32 {
                neighbors.push(ny as usize * width as usize + nx as usize);
            }
        }
    }
    neighbors
}

fn zhang_suen_thin(mut foreground: Vec<bool>, width: u32, height: u32) -> Vec<bool> {
    if width < 3 || height < 3 {
        return foreground;
    }
    let width = width as usize;
    let height = height as usize;
    for _ in 0..128 {
        let mut changed = false;
        for phase in 0..2 {
            let mut remove = Vec::new();
            for y in 1..height - 1 {
                for x in 1..width - 1 {
                    let index = y * width + x;
                    if !foreground[index] {
                        continue;
                    }
                    let p = [
                        foreground[index - width],
                        foreground[index - width + 1],
                        foreground[index + 1],
                        foreground[index + width + 1],
                        foreground[index + width],
                        foreground[index + width - 1],
                        foreground[index - 1],
                        foreground[index - width - 1],
                    ];
                    let count = p.iter().filter(|value| **value).count();
                    if !(2..=6).contains(&count) {
                        continue;
                    }
                    let transitions = (0..8)
                        .filter(|index| !p[*index] && p[(*index + 1) % 8])
                        .count();
                    if transitions != 1 {
                        continue;
                    }
                    let keep = if phase == 0 {
                        !(p[0] && p[2] && p[4]) && !(p[2] && p[4] && p[6])
                    } else {
                        !(p[0] && p[2] && p[6]) && !(p[0] && p[4] && p[6])
                    };
                    if keep {
                        remove.push(index);
                    }
                }
            }
            if !remove.is_empty() {
                changed = true;
                for index in remove {
                    foreground[index] = false;
                }
            }
        }
        if !changed {
            break;
        }
    }
    foreground
}

fn fit_primitives(
    skeleton: &[bool],
    width: u32,
    height: u32,
    millimeters_per_pixel: f64,
    tolerance_mm: f64,
) -> (Vec<DrawingPrimitive>, Vec<String>) {
    let mut visited = vec![false; skeleton.len()];
    let mut queue = Vec::new();
    let mut components = Vec::<Vec<usize>>::new();
    for start in 0..skeleton.len() {
        if !skeleton[start] || visited[start] {
            continue;
        }
        visited[start] = true;
        queue.clear();
        queue.push(start);
        let mut cursor = 0;
        while cursor < queue.len() {
            let current = queue[cursor];
            cursor += 1;
            for neighbor in pixel_neighbors(current, width, height) {
                if skeleton[neighbor] && !visited[neighbor] {
                    visited[neighbor] = true;
                    queue.push(neighbor);
                }
            }
        }
        components.push(queue.clone());
    }
    components.sort_by_key(|component| component.iter().copied().min().unwrap_or(usize::MAX));

    let mut primitives = Vec::new();
    let mut diagnostics = Vec::new();
    for component in components {
        if primitives.len() >= MAX_DRAWING_PRIMITIVES {
            diagnostics.push(format!("Recognition reached the {MAX_DRAWING_PRIMITIVES}-primitive limit; crop to one view region."));
            break;
        }
        if component.len() > MAX_COMPONENT_POINTS {
            diagnostics.push(format!("A connected line component exceeds the {MAX_COMPONENT_POINTS}-pixel processing limit and was left for manual repair."));
            continue;
        }
        if component.len() < 8 {
            continue;
        }
        let adjacency = component_adjacency(&component, width);
        let endpoints = adjacency
            .iter()
            .enumerate()
            .filter_map(|(index, neighbors)| (neighbors.len() == 1).then_some(index))
            .collect::<Vec<_>>();
        let is_closed =
            endpoints.is_empty() && adjacency.iter().all(|neighbors| neighbors.len() == 2);
        let pixel_points = component
            .iter()
            .map(|index| {
                [
                    (*index as u32 % width) as f64,
                    (*index as u32 / width) as f64,
                ]
            })
            .collect::<Vec<_>>();
        let id = format!("drawing-{:06}", primitives.len());
        if endpoints.is_empty() && !is_closed {
            if let Some((points, residual_px)) =
                fit_observed_polygon(&pixel_points, tolerance_mm / millimeters_per_pixel)
            {
                let points_mm = points
                    .iter()
                    .map(|point| {
                        to_view_mm(*point, width as f64, height as f64, millimeters_per_pixel)
                    })
                    .collect::<Vec<_>>();
                if points_mm.len() >= 3 && !loop_self_intersects(&points_mm) {
                    primitives.push(DrawingPrimitive::Polyline {
                        id,
                        points_mm,
                        closed: true,
                        residual_mm: residual_px * millimeters_per_pixel,
                    });
                    continue;
                }
            }
            diagnostics.push(
                "A centerline has a junction or ambiguous closure; it was left out of closed profiles.".into(),
            );
            continue;
        }
        if is_closed {
            if let Some((center, radius, residual)) = fit_circle(&pixel_points) {
                let residual_mm = residual * millimeters_per_pixel;
                if radius >= 4.0 && residual_mm <= tolerance_mm {
                    primitives.push(DrawingPrimitive::Circle {
                        id,
                        center_mm: to_view_mm(
                            center,
                            width as f64,
                            height as f64,
                            millimeters_per_pixel,
                        ),
                        radius_mm: radius * millimeters_per_pixel,
                        residual_mm,
                    });
                    continue;
                }
            }
            match trace_graph_path(&component, &adjacency, width, None) {
                Some(path) => {
                    let (simplified, residual_px) =
                        simplify_closed(&path, tolerance_mm / millimeters_per_pixel);
                    let points_mm = simplified
                        .iter()
                        .map(|point| {
                            to_view_mm(*point, width as f64, height as f64, millimeters_per_pixel)
                        })
                        .collect::<Vec<_>>();
                    if points_mm.len() >= 3 {
                        primitives.push(DrawingPrimitive::Polyline {
                            id,
                            points_mm,
                            closed: true,
                            residual_mm: residual_px * millimeters_per_pixel,
                        });
                    }
                }
                None => diagnostics
                    .push("A closed centerline has an ambiguous junction and needs review.".into()),
            }
        } else if endpoints.len() == 2
            && adjacency
                .iter()
                .all(|neighbors| (1..=2).contains(&neighbors.len()))
        {
            if let Some(path) = trace_graph_path(&component, &adjacency, width, Some(endpoints[0]))
            {
                let (simplified, residual_px) =
                    simplify_open(&path, tolerance_mm / millimeters_per_pixel);
                let points_mm = simplified
                    .iter()
                    .map(|point| {
                        to_view_mm(*point, width as f64, height as f64, millimeters_per_pixel)
                    })
                    .collect::<Vec<_>>();
                if points_mm.len() == 2 {
                    primitives.push(DrawingPrimitive::Line {
                        id,
                        start_mm: points_mm[0],
                        end_mm: points_mm[1],
                        residual_mm: residual_px * millimeters_per_pixel,
                    });
                } else if !points_mm.is_empty() {
                    primitives.push(DrawingPrimitive::Polyline {
                        id,
                        points_mm,
                        closed: false,
                        residual_mm: residual_px * millimeters_per_pixel,
                    });
                }
            }
        } else {
            diagnostics.push(format!(
                "A centerline has {} endpoints and a junction; it was left out of closed profiles.",
                endpoints.len()
            ));
        }
    }
    (primitives, diagnostics)
}

fn component_adjacency(component: &[usize], width: u32) -> Vec<Vec<usize>> {
    let membership = component
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    component
        .iter()
        .map(|index| {
            let x = (*index as u32 % width) as i32;
            let y = (*index as u32 / width) as i32;
            let mut neighbors = Vec::with_capacity(3);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let nx = x + dx;
                    let ny = y + dy;
                    if nx < 0 || ny < 0 {
                        continue;
                    }
                    let neighbor = ny as usize * width as usize + nx as usize;
                    if !membership.contains(&neighbor) {
                        continue;
                    }
                    if dx != 0 && dy != 0 {
                        let horizontal = y as usize * width as usize + nx as usize;
                        let vertical = ny as usize * width as usize + x as usize;
                        if membership.contains(&horizontal) || membership.contains(&vertical) {
                            continue;
                        }
                    }
                    neighbors.push(neighbor);
                }
            }
            neighbors
        })
        .collect()
}

#[derive(Clone, Copy)]
struct ObservedLine {
    center: [f64; 2],
    direction: [f64; 2],
    normal: [f64; 2],
    extent: [f64; 2],
    residual: f64,
}

fn fit_observed_polygon(points: &[[f64; 2]], tolerance_px: f64) -> Option<(Vec<[f64; 2]>, f64)> {
    if points.len() < 24 || points.len() > MAX_COMPONENT_POINTS {
        return None;
    }
    let line_tolerance = tolerance_px.min(1.5);
    let mut remaining = points.to_vec();
    let mut lines = Vec::new();
    let mut remaining_work = MAX_LINE_FIT_WORK;
    for _ in 0..16 {
        if remaining.len() < 10 {
            break;
        }
        let sample_stride = remaining.len().div_ceil(32).max(1);
        let sample = remaining
            .iter()
            .step_by(sample_stride.max(1))
            .copied()
            .take(32)
            .collect::<Vec<_>>();
        let estimated_work =
            sample.len().saturating_mul(sample.len().saturating_sub(1)) / 2 * remaining.len();
        if estimated_work > remaining_work {
            return None;
        }
        remaining_work -= estimated_work;
        let mut best: Option<([f64; 2], f64, usize)> = None;
        for left in 0..sample.len() {
            for right in (left + 1)..sample.len() {
                let delta = [
                    sample[right][0] - sample[left][0],
                    sample[right][1] - sample[left][1],
                ];
                let length = delta[0].hypot(delta[1]);
                if length < 8.0 {
                    continue;
                }
                let normal = [-delta[1] / length, delta[0] / length];
                let distance = normal[0] * sample[left][0] + normal[1] * sample[left][1];
                let support = remaining
                    .iter()
                    .filter(|point| {
                        (normal[0] * point[0] + normal[1] * point[1] - distance).abs()
                            <= line_tolerance
                    })
                    .count();
                if best.is_none_or(|(_, _, current)| support > current) {
                    best = Some((normal, distance, support));
                }
            }
        }
        let (normal, distance, support) = best?;
        if support < 12 {
            break;
        }
        let inliers = remaining
            .iter()
            .copied()
            .filter(|point| {
                (normal[0] * point[0] + normal[1] * point[1] - distance).abs() <= line_tolerance
            })
            .collect::<Vec<_>>();
        let center = [
            inliers.iter().map(|point| point[0]).sum::<f64>() / inliers.len() as f64,
            inliers.iter().map(|point| point[1]).sum::<f64>() / inliers.len() as f64,
        ];
        let (xx, xy, yy) = inliers.iter().fold((0.0, 0.0, 0.0), |(xx, xy, yy), point| {
            let dx = point[0] - center[0];
            let dy = point[1] - center[1];
            (xx + dx * dx, xy + dx * dy, yy + dy * dy)
        });
        let angle = 0.5 * (2.0 * xy).atan2(xx - yy);
        let direction = [angle.cos(), angle.sin()];
        let fitted_normal = [-direction[1], direction[0]];
        let mut projections = inliers
            .iter()
            .map(|point| {
                (point[0] - center[0]) * direction[0] + (point[1] - center[1]) * direction[1]
            })
            .collect::<Vec<_>>();
        projections.sort_by(f64::total_cmp);
        let min_projection = *projections.first()?;
        let max_projection = *projections.last()?;
        if max_projection - min_projection < 8.0 {
            break;
        }
        let residual = (inliers
            .iter()
            .map(|point| {
                (fitted_normal[0] * (point[0] - center[0])
                    + fitted_normal[1] * (point[1] - center[1]))
                    .powi(2)
            })
            .sum::<f64>()
            / inliers.len() as f64)
            .sqrt();
        lines.push(ObservedLine {
            center,
            direction,
            normal: fitted_normal,
            extent: [min_projection, max_projection],
            residual,
        });
        remaining.retain(|point| {
            (fitted_normal[0] * (point[0] - center[0]) + fitted_normal[1] * (point[1] - center[1]))
                .abs()
                > line_tolerance
        });
    }
    if !(3..=12).contains(&lines.len()) {
        return None;
    }

    let mut intersections = vec![vec![None; lines.len()]; lines.len()];
    let join_tolerance = tolerance_px;
    let mut join_error = 0.0f64;
    let mut adjacency = vec![Vec::new(); lines.len()];
    for left in 0..lines.len() {
        for right in (left + 1)..lines.len() {
            let a = lines[left];
            let b = lines[right];
            let determinant = a.normal[0] * b.normal[1] - a.normal[1] * b.normal[0];
            if determinant.abs() < 0.08 {
                continue;
            }
            let a_distance = a.normal[0] * a.center[0] + a.normal[1] * a.center[1];
            let b_distance = b.normal[0] * b.center[0] + b.normal[1] * b.center[1];
            let point = [
                (a_distance * b.normal[1] - a.normal[1] * b_distance) / determinant,
                (a.normal[0] * b_distance - a_distance * b.normal[0]) / determinant,
            ];
            let a_projection = (point[0] - a.center[0]) * a.direction[0]
                + (point[1] - a.center[1]) * a.direction[1];
            let b_projection = (point[0] - b.center[0]) * b.direction[0]
                + (point[1] - b.center[1]) * b.direction[1];
            if a_projection < a.extent[0] - join_tolerance
                || a_projection > a.extent[1] + join_tolerance
                || b_projection < b.extent[0] - join_tolerance
                || b_projection > b.extent[1] + join_tolerance
            {
                continue;
            }
            join_error = join_error
                .max(
                    (a.extent[0] - a_projection)
                        .max(a_projection - a.extent[1])
                        .max(0.0),
                )
                .max(
                    (b.extent[0] - b_projection)
                        .max(b_projection - b.extent[1])
                        .max(0.0),
                );
            intersections[left][right] = Some(point);
            intersections[right][left] = Some(point);
            adjacency[left].push(right);
            adjacency[right].push(left);
        }
    }
    if adjacency.iter().any(|neighbors| neighbors.len() != 2) {
        return None;
    }
    let mut order = Vec::with_capacity(lines.len());
    let mut previous = None;
    let mut current = 0;
    loop {
        if order.contains(&current) {
            if current != 0 || order.len() != lines.len() {
                return None;
            }
            break;
        }
        order.push(current);
        let next = adjacency[current]
            .iter()
            .copied()
            .find(|candidate| Some(*candidate) != previous)?;
        previous = Some(current);
        current = next;
    }
    let polygon = order
        .iter()
        .enumerate()
        .map(|(index, line)| intersections[*line][order[(index + 1) % order.len()]])
        .collect::<Option<Vec<_>>>()?;
    if polygon.len() < 3 || loop_self_intersects(&polygon) {
        return None;
    }
    let residual = lines
        .iter()
        .map(|line| line.residual)
        .fold(join_error, f64::max);
    if residual > tolerance_px {
        return None;
    }
    Some((polygon, residual))
}

fn trace_graph_path(
    component: &[usize],
    adjacency: &[Vec<usize>],
    width: u32,
    start: Option<usize>,
) -> Option<Vec<[f64; 2]>> {
    let local = component
        .iter()
        .enumerate()
        .map(|(index, pixel)| (*pixel, index))
        .collect::<std::collections::BTreeMap<_, _>>();
    if start.is_none() && adjacency.iter().any(|neighbors| neighbors.len() != 2) {
        return None;
    }
    let first = start.unwrap_or_else(|| {
        component
            .iter()
            .enumerate()
            .min_by_key(|(_, pixel)| **pixel)
            .map(|(index, _)| index)
            .unwrap_or(0)
    });
    let mut path = vec![first];
    let mut previous = None;
    let mut current = first;
    for _ in 0..=component.len() {
        let next = adjacency[current]
            .iter()
            .filter_map(|pixel| local.get(pixel).copied())
            .filter(|candidate| Some(*candidate) != previous)
            .find(|candidate| *candidate != first || path.len() > 2);
        let Some(next) = next else {
            break;
        };
        if next == first {
            if start.is_some() || path.len() == component.len() {
                return Some(
                    path.iter()
                        .map(|index| {
                            let pixel = component[*index];
                            [
                                (pixel % width as usize) as f64,
                                (pixel / width as usize) as f64,
                            ]
                        })
                        .collect(),
                );
            }
            break;
        }
        if path.contains(&next) {
            break;
        }
        path.push(next);
        previous = Some(current);
        current = next;
    }
    if path.len() != component.len() {
        return None;
    }
    Some(
        path.iter()
            .map(|index| {
                let pixel = component[*index];
                [
                    (pixel % width as usize) as f64,
                    (pixel / width as usize) as f64,
                ]
            })
            .collect(),
    )
}

fn fit_circle(points: &[[f64; 2]]) -> Option<([f64; 2], f64, f64)> {
    if points.len() < 24 {
        return None;
    }
    let mut normal = [[0.0f64; 4]; 3];
    for [x, y] in points {
        let row = [*x, *y, 1.0];
        let value = -x * x - y * y;
        for r in 0..3 {
            for c in 0..3 {
                normal[r][c] += row[r] * row[c];
            }
            normal[r][3] += row[r] * value;
        }
    }
    for pivot in 0..3 {
        let best = (pivot..3).max_by(|left, right| {
            normal[*left][pivot]
                .abs()
                .total_cmp(&normal[*right][pivot].abs())
        })?;
        if normal[best][pivot].abs() < 1.0e-10 {
            return None;
        }
        normal.swap(pivot, best);
        let divisor = normal[pivot][pivot];
        for col in pivot..4 {
            normal[pivot][col] /= divisor;
        }
        for row in 0..3 {
            if row == pivot {
                continue;
            }
            let factor = normal[row][pivot];
            for col in pivot..4 {
                normal[row][col] -= factor * normal[pivot][col];
            }
        }
    }
    let a = normal[0][3];
    let b = normal[1][3];
    let c = normal[2][3];
    let center = [-a / 2.0, -b / 2.0];
    let radius_sq = center[0] * center[0] + center[1] * center[1] - c;
    if radius_sq <= 0.0 {
        return None;
    }
    let radius = radius_sq.sqrt();
    let residual = (points
        .iter()
        .map(|point| ((point[0] - center[0]).hypot(point[1] - center[1]) - radius).powi(2))
        .sum::<f64>()
        / points.len() as f64)
        .sqrt();
    let mut angles = points
        .iter()
        .map(|point| {
            (point[1] - center[1])
                .atan2(point[0] - center[0])
                .rem_euclid(std::f64::consts::TAU)
        })
        .collect::<Vec<_>>();
    angles.sort_by(f64::total_cmp);
    let max_gap = (0..angles.len())
        .map(|index| {
            let next = if index + 1 == angles.len() {
                angles[0] + std::f64::consts::TAU
            } else {
                angles[index + 1]
            };
            next - angles[index]
        })
        .fold(0.0, f64::max);
    (max_gap < 0.5).then_some((center, radius, residual))
}

fn simplify_open(path: &[[f64; 2]], tolerance: f64) -> (Vec<[f64; 2]>, f64) {
    simplify_rdp(path, tolerance)
}

fn simplify_closed(path: &[[f64; 2]], tolerance: f64) -> (Vec<[f64; 2]>, f64) {
    if path.len() < 4 {
        return (path.to_vec(), 0.0);
    }
    let start = 0;
    let opposite = (1..path.len())
        .max_by(|left, right| {
            distance_sq(path[start], path[*left]).total_cmp(&distance_sq(path[start], path[*right]))
        })
        .unwrap_or(path.len() / 2);
    let first = path[start..=opposite].to_vec();
    let mut second = path[opposite..].to_vec();
    second.extend_from_slice(&path[..=start]);
    let (mut left, left_residual) = simplify_rdp(&first, tolerance);
    let (right, right_residual) = simplify_rdp(&second, tolerance);
    if !right.is_empty() {
        left.extend(right.into_iter().skip(1));
    }
    if left.last() == left.first() {
        left.pop();
    }
    (left, left_residual.max(right_residual))
}

fn simplify_rdp(points: &[[f64; 2]], tolerance: f64) -> (Vec<[f64; 2]>, f64) {
    if points.len() <= 2 {
        return (points.to_vec(), 0.0);
    }
    let mut max_distance = 0.0;
    let mut split = 0;
    for index in 1..points.len() - 1 {
        let distance = point_segment_distance(points[index], points[0], points[points.len() - 1]);
        if distance > max_distance {
            max_distance = distance;
            split = index;
        }
    }
    if max_distance > tolerance {
        let (mut left, left_residual) = simplify_rdp(&points[..=split], tolerance);
        let (right, right_residual) = simplify_rdp(&points[split..], tolerance);
        left.extend(right.into_iter().skip(1));
        (left, left_residual.max(right_residual))
    } else {
        (vec![points[0], points[points.len() - 1]], max_distance)
    }
}

fn point_segment_distance(point: [f64; 2], start: [f64; 2], end: [f64; 2]) -> f64 {
    let dx = end[0] - start[0];
    let dy = end[1] - start[1];
    let length_sq = dx * dx + dy * dy;
    if length_sq <= 1.0e-12 {
        return distance_sq(point, start).sqrt();
    }
    let t = (((point[0] - start[0]) * dx + (point[1] - start[1]) * dy) / length_sq).clamp(0.0, 1.0);
    distance_sq(point, [start[0] + t * dx, start[1] + t * dy]).sqrt()
}

fn distance_sq(left: [f64; 2], right: [f64; 2]) -> f64 {
    (left[0] - right[0]).powi(2) + (left[1] - right[1]).powi(2)
}

fn to_view_mm(point: [f64; 2], _width: f64, height: f64, scale: f64) -> [f64; 2] {
    [point[0] * scale, (height - point[1]) * scale]
}

pub(crate) fn assemble_profile_candidates(
    primitives: &[DrawingPrimitive],
) -> Vec<DrawingProfileCandidate> {
    let loops = primitives
        .iter()
        .filter_map(|primitive| match primitive {
            DrawingPrimitive::Circle {
                id,
                center_mm,
                radius_mm,
                residual_mm,
            } => Some((id.as_str(), vec![*center_mm], *radius_mm, *residual_mm)),
            DrawingPrimitive::Polyline {
                id,
                points_mm,
                closed: true,
                residual_mm,
            } => Some((id.as_str(), points_mm.clone(), 0.0, *residual_mm)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let conflicts = (0..loops.len())
        .flat_map(|left| ((left + 1)..loops.len()).map(move |right| (left, right)))
        .filter(|(left, right)| shape_boundaries_conflict(&loops[*left], &loops[*right]))
        .flat_map(|(left, right)| [(left, right), (right, left)])
        .collect::<std::collections::BTreeSet<_>>();
    let mut result = Vec::new();
    for (index, (id, points, radius, residual)) in loops.iter().enumerate() {
        let containers = loops
            .iter()
            .enumerate()
            .filter(|(other_index, (other_id, other_points, other_radius, _))| {
                *other_index != index
                    && loop_area(other_points, *other_radius) > loop_area(points, *radius)
                    && shape_contains(
                        (other_points.as_slice(), *other_radius),
                        (points.as_slice(), *radius),
                    )
                    && other_id != id
            })
            .map(|(other_index, _)| other_index)
            .collect::<Vec<_>>();
        let parent_index = containers.iter().copied().min_by(|left, right| {
            loop_area(&loops[*left].1, loops[*left].2)
                .total_cmp(&loop_area(&loops[*right].1, loops[*right].2))
        });
        let mut depth = 0;
        let mut cursor = parent_index;
        while let Some(parent) = cursor {
            depth += 1;
            cursor = loops
                .iter()
                .enumerate()
                .filter(|(other, (_, candidate, candidate_radius, _))| {
                    *other != parent
                        && loop_area(candidate, *candidate_radius)
                            > loop_area(&loops[parent].1, loops[parent].2)
                        && shape_contains(
                            (candidate.as_slice(), *candidate_radius),
                            (loops[parent].1.as_slice(), loops[parent].2),
                        )
                })
                .map(|(other, _)| other)
                .min_by(|left, right| {
                    loop_area(&loops[*left].1, loops[*left].2)
                        .total_cmp(&loop_area(&loops[*right].1, loops[*right].2))
                });
        }
        let role = if depth % 2 == 0 {
            DrawingProfileRole::Outer
        } else {
            DrawingProfileRole::Hole
        };
        result.push(DrawingProfileCandidate {
            id: format!("profile-{id}"),
            primitive_ids: vec![(*id).to_string()],
            role,
            parent_id: parent_index.map(|parent| format!("profile-{}", loops[parent].0)),
            closed: true,
            residual_mm: *residual,
            issues: if conflicts.iter().any(|(conflict, _)| *conflict == index) {
                vec!["This boundary crosses or touches another profile; separate the geometry before applying.".into()]
            } else if *radius == 0.0 && loop_self_intersects(points) {
                vec!["This outline crosses itself; repair it before applying.".into()]
            } else {
                Vec::new()
            },
        });
    }
    result
}

pub(crate) struct DrawingGeometryReview {
    pub closed_primitives: Vec<DrawingPrimitive>,
    pub profiles: Vec<DrawingProfileCandidate>,
    pub diagnostics: Vec<String>,
}

pub(crate) fn review_profile_primitives(primitives: &[DrawingPrimitive]) -> DrawingGeometryReview {
    let mut diagnostics = Vec::new();
    if primitives.len() > MAX_DRAWING_PRIMITIVES {
        return DrawingGeometryReview {
            closed_primitives: Vec::new(),
            profiles: Vec::new(),
            diagnostics: vec![format!("Profile review supports at most {MAX_DRAWING_PRIMITIVES} primitives; remove extra geometry.")],
        };
    }
    let point_count = primitives.iter().fold(0usize, |total, primitive| {
        total.saturating_add(match primitive {
            DrawingPrimitive::Line { .. } => 2,
            DrawingPrimitive::Circle { .. } => 1,
            DrawingPrimitive::Polyline { points_mm, .. } => points_mm.len(),
        })
    });
    if point_count > MAX_REVIEW_POINTS {
        return DrawingGeometryReview {
            closed_primitives: Vec::new(),
            profiles: Vec::new(),
            diagnostics: vec![format!("Profile review supports at most {MAX_REVIEW_POINTS} points; simplify extra geometry.")],
        };
    }
    let mut ids = std::collections::BTreeSet::new();
    let mut closed = Vec::new();
    let mut source_ids_by_loop = std::collections::BTreeMap::<String, Vec<String>>::new();
    let mut segments = Vec::<(String, [f64; 2], [f64; 2], f64)>::new();
    for primitive in primitives {
        let id = match primitive {
            DrawingPrimitive::Line { id, .. }
            | DrawingPrimitive::Circle { id, .. }
            | DrawingPrimitive::Polyline { id, .. } => id,
        };
        if id.trim().is_empty() || !ids.insert(id.clone()) {
            diagnostics.push("Drawing primitive identifiers must be non-empty and unique.".into());
            continue;
        }
        match primitive {
            DrawingPrimitive::Line {
                start_mm,
                end_mm,
                residual_mm,
                ..
            } => {
                if !valid_fit_residual(*residual_mm)
                    || !valid_point(*start_mm)
                    || !valid_point(*end_mm)
                    || distance_sq(*start_mm, *end_mm) <= 1.0e-12
                {
                    diagnostics.push(format!(
                        "Drawing line '{id}' has invalid endpoints or residual."
                    ));
                    continue;
                }
                segments.push((id.clone(), *start_mm, *end_mm, *residual_mm));
            }
            DrawingPrimitive::Circle {
                center_mm,
                radius_mm,
                residual_mm,
                ..
            } => {
                if !valid_point(*center_mm)
                    || !radius_mm.is_finite()
                    || !(0.0..=1.0e6).contains(radius_mm)
                    || *radius_mm == 0.0
                    || !valid_fit_residual(*residual_mm)
                {
                    diagnostics.push(format!(
                        "Drawing circle '{id}' has invalid center, radius, or residual."
                    ));
                    continue;
                }
                closed.push(primitive.clone());
            }
            DrawingPrimitive::Polyline {
                points_mm,
                closed: is_closed,
                residual_mm,
                ..
            } => {
                if !valid_fit_residual(*residual_mm)
                    || points_mm.iter().any(|point| !valid_point(*point))
                    || points_mm.len() > MAX_REVIEW_POINTS
                    || points_mm.len() < if *is_closed { 3 } else { 2 }
                    || points_mm
                        .windows(2)
                        .any(|pair| distance_sq(pair[0], pair[1]) <= 1.0e-12)
                {
                    diagnostics.push(format!(
                        "Drawing outline '{id}' has invalid points or residual."
                    ));
                    continue;
                }
                if *is_closed {
                    let mut points = points_mm.clone();
                    if points.first() == points.last() {
                        points.pop();
                    }
                    if points.len() < 3 || loop_self_intersects(&points) {
                        diagnostics.push(format!(
                            "Drawing outline '{id}' is degenerate or crosses itself."
                        ));
                        continue;
                    }
                    closed.push(DrawingPrimitive::Polyline {
                        id: id.clone(),
                        points_mm: points,
                        closed: true,
                        residual_mm: *residual_mm,
                    });
                } else {
                    segments.extend(
                        points_mm
                            .windows(2)
                            .map(|pair| (id.clone(), pair[0], pair[1], *residual_mm)),
                    );
                }
            }
        }
    }

    let mut endpoint_edges =
        std::collections::BTreeMap::<(i64, i64), Vec<(usize, [f64; 2])>>::new();
    for (index, (_, start, end, _)) in segments.iter().enumerate() {
        endpoint_edges
            .entry(drawing_endpoint_key(*start))
            .or_default()
            .push((index, *start));
        endpoint_edges
            .entry(drawing_endpoint_key(*end))
            .or_default()
            .push((index, *end));
    }
    let mut segment_neighbors = vec![Vec::<usize>::new(); segments.len()];
    for edges in endpoint_edges.values() {
        for (index, _) in edges {
            segment_neighbors[*index].extend(
                edges
                    .iter()
                    .map(|(neighbor, _)| *neighbor)
                    .filter(|neighbor| neighbor != index),
            );
        }
    }
    let mut seen = vec![false; segments.len()];
    for start in 0..segments.len() {
        if seen[start] {
            continue;
        }
        let mut component = Vec::new();
        let mut queue = vec![start];
        seen[start] = true;
        let mut cursor = 0;
        while cursor < queue.len() {
            let current = queue[cursor];
            cursor += 1;
            component.push(current);
            for next in &segment_neighbors[current] {
                if !seen[*next] {
                    seen[*next] = true;
                    queue.push(*next);
                }
            }
        }
        let component_set = component
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        let keys = component
            .iter()
            .flat_map(|index| {
                [
                    drawing_endpoint_key(segments[*index].1),
                    drawing_endpoint_key(segments[*index].2),
                ]
            })
            .collect::<std::collections::BTreeSet<_>>();
        if keys.iter().any(|key| {
            endpoint_edges.get(key).is_none_or(|edges| {
                edges
                    .iter()
                    .filter(|(edge, _)| component_set.contains(edge))
                    .count()
                    != 2
            })
        }) {
            diagnostics.push("Drawing line chain is open or branched; repair endpoints or remove it before applying.".into());
            continue;
        }
        let first_segment = component[0];
        let start_point = segments[first_segment].1;
        let start_key = drawing_endpoint_key(start_point);
        let mut current_key = start_key;
        let mut current_segment = first_segment;
        let mut used = std::collections::BTreeSet::new();
        let mut points = Vec::new();
        let mut residual = 0.0f64;
        loop {
            if !used.insert(current_segment) {
                break;
            }
            let (_, start_point, end_point, segment_residual) = &segments[current_segment];
            let (start_point, end_point, segment_residual) =
                (*start_point, *end_point, *segment_residual);
            residual = residual.max(segment_residual);
            let (from, to) = if drawing_endpoint_key(start_point) == current_key {
                (start_point, end_point)
            } else {
                (end_point, start_point)
            };
            if points.is_empty() {
                points.push(from);
            }
            points.push(to);
            current_key = drawing_endpoint_key(to);
            if current_key == start_key {
                break;
            }
            let next = endpoint_edges.get(&current_key).and_then(|edges| {
                edges
                    .iter()
                    .map(|(edge, _)| *edge)
                    .find(|edge| *edge != current_segment && !used.contains(edge))
            });
            let Some(next) = next else {
                break;
            };
            current_segment = next;
        }
        if used.len() != component.len() || points.len() < 4 || current_key != start_key {
            diagnostics.push(
                "Drawing line chain did not form one closed contour; repair it before applying."
                    .into(),
            );
            continue;
        }
        points.pop();
        if loop_self_intersects(&points) {
            diagnostics.push("Repaired drawing line contour crosses itself.".into());
            continue;
        }
        canonicalize_profile_points(&mut points);
        let mut source_ids = component
            .iter()
            .map(|index| segments[*index].0.clone())
            .collect::<Vec<_>>();
        source_ids.sort();
        source_ids.dedup();
        let normalized_id = format!("drawing-repaired-{}", source_ids.join("|"));
        source_ids_by_loop.insert(normalized_id.clone(), source_ids);
        closed.push(DrawingPrimitive::Polyline {
            id: normalized_id,
            points_mm: points,
            closed: true,
            residual_mm: residual,
        });
    }

    let mut profiles = assemble_profile_candidates(&closed);
    let mut replacement_ids = std::collections::BTreeMap::new();
    for profile in &mut profiles {
        if let Some(normalized_id) = profile.primitive_ids.first() {
            if let Some(source_ids) = source_ids_by_loop.get(normalized_id) {
                let new_id = format!("profile-{}", source_ids.join("|"));
                replacement_ids.insert(profile.id.clone(), new_id.clone());
                profile.id = new_id;
                profile.primitive_ids = source_ids.clone();
            }
        }
    }
    for profile in &mut profiles {
        if let Some(parent_id) = profile
            .parent_id
            .as_ref()
            .and_then(|id| replacement_ids.get(id))
        {
            profile.parent_id = Some(parent_id.clone());
        }
    }
    for profile in &profiles {
        diagnostics.extend(profile.issues.iter().cloned());
    }
    if !profiles
        .iter()
        .any(|profile| profile.role == DrawingProfileRole::Outer && profile.issues.is_empty())
    {
        diagnostics.push("Drawing geometry needs at least one valid closed outer outline.".into());
    }
    diagnostics.sort();
    diagnostics.dedup();
    DrawingGeometryReview {
        closed_primitives: closed,
        profiles,
        diagnostics,
    }
}

pub(crate) fn validate_accepted_profile_primitives(
    primitives: &[DrawingPrimitive],
) -> AppResult<Vec<DrawingPrimitive>> {
    let reviewed = review_profile_primitives(primitives);
    if let Some(diagnostic) = reviewed.diagnostics.first() {
        return Err(AppError::validation(diagnostic.clone()));
    }
    Ok(reviewed.closed_primitives)
}

fn valid_point(point: [f64; 2]) -> bool {
    point
        .iter()
        .all(|value| value.is_finite() && value.abs() <= 1.0e6)
}

fn valid_fit_residual(residual: f64) -> bool {
    residual.is_finite() && (0.0..=1.0e6).contains(&residual)
}

fn canonicalize_profile_points(points: &mut [[f64; 2]]) {
    if points.len() < 3 {
        return;
    }
    if let Some(min_index) = (0..points.len()).min_by(|left, right| {
        points[*left][0]
            .total_cmp(&points[*right][0])
            .then_with(|| points[*left][1].total_cmp(&points[*right][1]))
    }) {
        points.rotate_left(min_index);
    }
}

fn drawing_endpoint_key(point: [f64; 2]) -> (i64, i64) {
    (
        (point[0] * 100_000.0).round() as i64,
        (point[1] * 100_000.0).round() as i64,
    )
}

fn shape_contains(container: (&[[f64; 2]], f64), child: (&[[f64; 2]], f64)) -> bool {
    let (container_points, container_radius) = container;
    let (child_points, child_radius) = child;
    const EPSILON: f64 = 1.0e-7;
    if container_radius > 0.0 {
        if child_radius > 0.0 {
            return distance_sq(container_points[0], child_points[0]).sqrt() + child_radius
                < container_radius - EPSILON;
        }
        return child_points.iter().all(|point| {
            distance_sq(container_points[0], *point).sqrt() < container_radius - EPSILON
        });
    }
    if child_radius > 0.0 {
        let center = child_points[0];
        return point_in_loop(center, container_points, 0.0)
            && (0..container_points.len()).all(|index| {
                distance_to_segment(
                    center,
                    container_points[index],
                    container_points[(index + 1) % container_points.len()],
                ) > child_radius + EPSILON
            });
    }
    child_points
        .iter()
        .all(|point| point_in_loop(*point, container_points, 0.0))
}

fn shape_boundaries_conflict(
    left: &(&str, Vec<[f64; 2]>, f64, f64),
    right: &(&str, Vec<[f64; 2]>, f64, f64),
) -> bool {
    const EPSILON: f64 = 1.0e-6;
    let (left_points, left_radius) = (&left.1, left.2);
    let (right_points, right_radius) = (&right.1, right.2);
    match (left_radius > 0.0, right_radius > 0.0) {
        (true, true) => {
            let distance = distance_sq(left_points[0], right_points[0]).sqrt();
            let minimum = (left_radius - right_radius).abs();
            let maximum = left_radius + right_radius;
            distance >= minimum - EPSILON && distance <= maximum + EPSILON
        }
        (true, false) => {
            circle_polygon_boundaries_conflict(left_points[0], left_radius, right_points)
        }
        (false, true) => {
            circle_polygon_boundaries_conflict(right_points[0], right_radius, left_points)
        }
        (false, false) => (0..left_points.len()).any(|left_index| {
            let left_start = left_points[left_index];
            let left_end = left_points[(left_index + 1) % left_points.len()];
            (0..right_points.len()).any(|right_index| {
                segments_touch(
                    left_start,
                    left_end,
                    right_points[right_index],
                    right_points[(right_index + 1) % right_points.len()],
                    EPSILON,
                )
            })
        }),
    }
}

fn circle_polygon_boundaries_conflict(center: [f64; 2], radius: f64, polygon: &[[f64; 2]]) -> bool {
    const EPSILON: f64 = 1.0e-6;
    (0..polygon.len()).any(|index| {
        let start = polygon[index];
        let end = polygon[(index + 1) % polygon.len()];
        let minimum = distance_to_segment(center, start, end);
        let maximum = distance_sq(center, start)
            .sqrt()
            .max(distance_sq(center, end).sqrt());
        minimum <= radius + EPSILON && maximum >= radius - EPSILON
    })
}

fn segments_touch(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2], epsilon: f64) -> bool {
    let orientation = |p: [f64; 2], q: [f64; 2], r: [f64; 2]| {
        (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])
    };
    let ab_c = orientation(a, b, c);
    let ab_d = orientation(a, b, d);
    let cd_a = orientation(c, d, a);
    let cd_b = orientation(c, d, b);
    if ((ab_c > epsilon && ab_d < -epsilon) || (ab_c < -epsilon && ab_d > epsilon))
        && ((cd_a > epsilon && cd_b < -epsilon) || (cd_a < -epsilon && cd_b > epsilon))
    {
        return true;
    }
    distance_to_segment(a, c, d) <= epsilon
        || distance_to_segment(b, c, d) <= epsilon
        || distance_to_segment(c, a, b) <= epsilon
        || distance_to_segment(d, a, b) <= epsilon
}

fn distance_to_segment(point: [f64; 2], start: [f64; 2], end: [f64; 2]) -> f64 {
    let delta = [end[0] - start[0], end[1] - start[1]];
    let length_sq = delta[0] * delta[0] + delta[1] * delta[1];
    if length_sq <= 1.0e-12 {
        return distance_sq(point, start).sqrt();
    }
    let fraction = (((point[0] - start[0]) * delta[0] + (point[1] - start[1]) * delta[1])
        / length_sq)
        .clamp(0.0, 1.0);
    distance_sq(
        point,
        [
            start[0] + fraction * delta[0],
            start[1] + fraction * delta[1],
        ],
    )
    .sqrt()
}

fn loop_self_intersects(points: &[[f64; 2]]) -> bool {
    if points.len() < 3 {
        return true;
    }
    for left in 0..points.len() {
        let left_next = (left + 1) % points.len();
        for right in (left + 1)..points.len() {
            let right_next = (right + 1) % points.len();
            if left == right || left_next == right || right_next == left {
                continue;
            }
            if segments_touch(
                points[left],
                points[left_next],
                points[right],
                points[right_next],
                1.0e-9,
            ) {
                return true;
            }
        }
    }
    false
}

fn loop_area(points: &[[f64; 2]], circle_radius: f64) -> f64 {
    if circle_radius > 0.0 {
        std::f64::consts::PI * circle_radius.powi(2)
    } else {
        polygon_area(points).abs()
    }
}

fn polygon_area(points: &[[f64; 2]]) -> f64 {
    (0..points.len())
        .map(|index| {
            let next = (index + 1) % points.len();
            points[index][0] * points[next][1] - points[next][0] * points[index][1]
        })
        .sum::<f64>()
        / 2.0
}

fn point_in_loop(point: [f64; 2], points: &[[f64; 2]], circle_radius: f64) -> bool {
    if circle_radius > 0.0 {
        return distance_sq(point, points[0]) < circle_radius.powi(2);
    }
    if points.len() < 3 {
        return false;
    }
    let mut inside = false;
    for index in 0..points.len() {
        let current = points[index];
        let previous = points[(index + points.len() - 1) % points.len()];
        if ((current[1] > point[1]) != (previous[1] > point[1]))
            && point[0]
                < (previous[0] - current[0]) * (point[1] - current[1]) / (previous[1] - current[1])
                    + current[0]
        {
            inside = !inside;
        }
    }
    inside
}

pub(crate) fn load_evidence(
    app_data_dir: &Path,
    evidence_id: &str,
) -> AppResult<DrawingImportEvidence> {
    let digest = evidence_id.strip_prefix("sha256:").ok_or_else(|| {
        AppError::validation("Drawing evidence identifier is invalid; recognize the image again.")
    })?;
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AppError::validation(
            "Drawing evidence identifier is invalid; recognize the image again.",
        ));
    }
    let path = app_data_dir
        .join("drawing-import")
        .join("evidence")
        .join(format!("{digest}.json"));
    let bytes = std::fs::read(path).map_err(|error| {
        AppError::validation(format!(
            "Drawing recognition evidence is unavailable: {error}"
        ))
    })?;
    if format!("sha256:{:x}", Sha256::digest(&bytes)) != evidence_id {
        return Err(AppError::conflict(
            "Drawing evidence changed after recognition. Recognize the image again.",
        ));
    }
    serde_json::from_slice(&bytes).map_err(|error| {
        AppError::validation(format!("Drawing recognition evidence is invalid: {error}"))
    })
}

#[allow(dead_code)]
fn _managed_directory(app: &dyn PathResolver) -> PathBuf {
    app.app_data_dir().join("drawing-import")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use image::{GrayImage, ImageFormat, Luma};
    use std::path::PathBuf;

    struct Resolver(PathBuf);

    impl PathResolver for Resolver {
        fn app_config_dir(&self) -> PathBuf {
            self.0.clone()
        }
        fn app_data_dir(&self) -> PathBuf {
            self.0.clone()
        }
        fn resource_path(&self, _path: &str) -> Option<PathBuf> {
            None
        }
    }

    pub(crate) fn plate_fixture(path: &Path, perspective: bool, noisy: bool) {
        let mut image = GrayImage::from_pixel(640, 420, Luma([255]));
        fn draw_disk(image: &mut GrayImage, cx: f64, cy: f64, radius: i32) {
            for y in (cy as i32 - radius)..=(cy as i32 + radius) {
                for x in (cx as i32 - radius)..=(cx as i32 + radius) {
                    if (x as f64 - cx).powi(2) + (y as f64 - cy).powi(2) <= (radius as f64).powi(2)
                        && x >= 0
                        && y >= 0
                        && x < 640
                        && y < 420
                    {
                        image.put_pixel(x as u32, y as u32, Luma([20]));
                    }
                }
            }
        }
        fn draw_line(image: &mut GrayImage, start: [f64; 2], end: [f64; 2], thickness: i32) {
            let steps = ((end[0] - start[0]).hypot(end[1] - start[1]) * 2.0).ceil() as usize;
            for index in 0..=steps {
                let t = index as f64 / steps.max(1) as f64;
                draw_disk(
                    image,
                    start[0] + (end[0] - start[0]) * t,
                    start[1] + (end[1] - start[1]) * t,
                    thickness / 2,
                );
            }
        }
        let corners = if perspective {
            [[112.0, 94.0], [522.0, 112.0], [490.0, 380.0], [94.0, 348.0]]
        } else {
            [
                [120.0, 100.0],
                [520.0, 100.0],
                [520.0, 350.0],
                [120.0, 350.0],
            ]
        };
        for index in 0..4 {
            draw_line(&mut image, corners[index], corners[(index + 1) % 4], 6);
        }
        let test_transform = perspective.then(|| fixture_projective_map(corners));
        for step in 0..720 {
            let angle = std::f64::consts::TAU * step as f64 / 720.0;
            let canonical = if perspective {
                [200.0 + 25.0 * angle.cos(), 125.0 + 25.0 * angle.sin()]
            } else {
                [320.0 + 25.0 * angle.cos(), 225.0 + 25.0 * angle.sin()]
            };
            let point = if let Some(transform) = test_transform {
                fixture_map_point(transform, [canonical[0] / 400.0, canonical[1] / 250.0])
            } else {
                canonical
            };
            draw_disk(&mut image, point[0], point[1], 3);
        }
        // Dimension-like marks outside selected view region must be ignored.
        draw_line(&mut image, [120.0, 35.0], [520.0, 35.0], 2);
        draw_line(&mut image, [115.0, 28.0], [115.0, 42.0], 2);
        if noisy {
            let mut seed = 17u32;
            for _ in 0..800 {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let x = seed % 640;
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let y = seed % 420;
                if image.get_pixel(x, y)[0] == 255 && seed % 3 == 0 {
                    image.put_pixel(x, y, Luma([120]));
                }
            }
        }
        image.save_with_format(path, ImageFormat::Png).unwrap();
    }

    fn fixture_projective_map(corners: [[f64; 2]; 4]) -> [f64; 9] {
        let [p0, p1, p2, p3] = corners;
        let dx1 = p1[0] - p2[0];
        let dx2 = p3[0] - p2[0];
        let dx3 = p0[0] - p1[0] + p2[0] - p3[0];
        let dy1 = p1[1] - p2[1];
        let dy2 = p3[1] - p2[1];
        let dy3 = p0[1] - p1[1] + p2[1] - p3[1];
        let denominator = dx1 * dy2 - dx2 * dy1;
        let (g, h) = if dx3.abs() < 1.0e-10 && dy3.abs() < 1.0e-10 {
            (0.0, 0.0)
        } else {
            (
                (dx3 * dy2 - dx2 * dy3) / denominator,
                (dx1 * dy3 - dx3 * dy1) / denominator,
            )
        };
        let a = p1[0] - p0[0] + g * p1[0];
        let b = p3[0] - p0[0] + h * p3[0];
        let d = p1[1] - p0[1] + g * p1[1];
        let e = p3[1] - p0[1] + h * p3[1];
        [a, b, p0[0], d, e, p0[1], g, h, 1.0]
    }

    fn fixture_map_point(matrix: [f64; 9], point: [f64; 2]) -> [f64; 2] {
        let denominator = matrix[6] * point[0] + matrix[7] * point[1] + 1.0;
        [
            (matrix[0] * point[0] + matrix[1] * point[1] + matrix[2]) / denominator,
            (matrix[3] * point[0] + matrix[4] * point[1] + matrix[5]) / denominator,
        ]
    }

    #[test]
    fn given_concave_cycle_when_fitted_then_preserves_traced_order() {
        let (width, height) = (20u32, 20u32);
        let mut skeleton = vec![false; (width * height) as usize];
        let vertices: [[i32; 2]; 6] = [[2, 2], [16, 2], [16, 5], [6, 5], [6, 16], [2, 16]];
        for index in 0..vertices.len() {
            let [x0, y0] = vertices[index];
            let [x1, y1] = vertices[(index + 1) % vertices.len()];
            let steps = (x1 - x0).abs().max((y1 - y0).abs());
            for step in 0..=steps {
                let x = x0 + (x1 - x0) * step / steps;
                let y = y0 + (y1 - y0) * step / steps;
                skeleton[y as usize * width as usize + x as usize] = true;
            }
        }
        let (primitives, diagnostics) = fit_primitives(&skeleton, width, height, 1.0, 0.25);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let [DrawingPrimitive::Polyline {
            points_mm,
            closed: true,
            ..
        }] = primitives.as_slice()
        else {
            panic!("concave cycle must yield one traced closed contour: {primitives:?}");
        };
        assert!(points_mm.len() >= 5);
        assert!(!loop_self_intersects(points_mm));
    }

    #[test]
    fn given_branched_centerline_when_fitted_then_never_invents_a_closed_outline() {
        let (width, height) = (20u32, 20u32);
        let mut skeleton = vec![false; (width * height) as usize];
        for x in 2..=14 {
            skeleton[5 * width as usize + x] = true;
        }
        for y in 5..=16 {
            skeleton[y * width as usize + 8] = true;
        }
        let (primitives, diagnostics) = fit_primitives(&skeleton, width, height, 1.0, 0.5);
        assert!(!primitives.iter().any(|primitive| matches!(
            primitive,
            DrawingPrimitive::Polyline { closed: true, .. } | DrawingPrimitive::Circle { .. }
        )));
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("junction")));
    }

    #[test]
    fn given_touching_or_crossing_profiles_when_candidates_assembled_then_marks_ambiguity() {
        let touching_circles = vec![
            DrawingPrimitive::Circle {
                id: "left".into(),
                center_mm: [0.0, 0.0],
                radius_mm: 5.0,
                residual_mm: 0.0,
            },
            DrawingPrimitive::Circle {
                id: "right".into(),
                center_mm: [10.0, 0.0],
                radius_mm: 5.0,
                residual_mm: 0.0,
            },
        ];
        let touching = assemble_profile_candidates(&touching_circles);
        assert!(touching.iter().all(|profile| !profile.issues.is_empty()));

        let crossing = vec![
            DrawingPrimitive::Polyline {
                id: "outline".into(),
                points_mm: vec![[-5.0, -5.0], [5.0, -5.0], [5.0, 5.0], [-5.0, 5.0]],
                closed: true,
                residual_mm: 0.0,
            },
            DrawingPrimitive::Circle {
                id: "crossing".into(),
                center_mm: [5.0, 0.0],
                radius_mm: 3.0,
                residual_mm: 0.0,
            },
        ];
        let crossing = assemble_profile_candidates(&crossing);
        assert!(crossing.iter().all(|profile| !profile.issues.is_empty()));
    }

    #[test]
    fn given_open_profile_then_repaired_line_when_reviewed_then_recomputes_outer_and_hole_roles() {
        let open = DrawingPrimitive::Polyline {
            id: "plate-lines".into(),
            points_mm: vec![[10.0, 10.0], [90.0, 10.0], [90.0, 60.0], [10.0, 60.0]],
            closed: false,
            residual_mm: 0.0,
        };
        let circle = DrawingPrimitive::Circle {
            id: "hole-circle".into(),
            center_mm: [50.0, 35.0],
            radius_mm: 5.0,
            residual_mm: 0.0,
        };
        let before = review_profile_primitives(&[open.clone(), circle.clone()]);
        assert!(before
            .profiles
            .iter()
            .any(|profile| profile.primitive_ids == ["hole-circle"]));
        assert!(before
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("open or branched")));

        let repaired = [
            open,
            circle,
            DrawingPrimitive::Line {
                id: "closing-edge".into(),
                start_mm: [10.0, 60.0],
                end_mm: [10.0, 10.0],
                residual_mm: 0.0,
            },
        ];
        let after = review_profile_primitives(&repaired);
        assert!(after.diagnostics.is_empty(), "{:?}", after.diagnostics);
        let outer = after
            .profiles
            .iter()
            .find(|profile| profile.role == DrawingProfileRole::Outer)
            .unwrap();
        let hole = after
            .profiles
            .iter()
            .find(|profile| profile.role == DrawingProfileRole::Hole)
            .unwrap();
        assert_eq!(outer.primitive_ids, ["closing-edge", "plate-lines"]);
        assert_eq!(hole.primitive_ids, ["hole-circle"]);
        assert_eq!(hole.parent_id.as_deref(), Some(outer.id.as_str()));
    }

    #[test]
    fn given_plate_raster_when_prepared_and_recognized_then_returns_centerline_polygon_and_analytic_hole(
    ) {
        let root = std::env::temp_dir().join(format!("ecky-drawing-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("plate.png");
        plate_fixture(&source, false, true);
        let resolver = Resolver(root.clone());
        let prepared = prepare_drawing_image(
            PrepareDrawingImageRequest {
                source_path: source.to_string_lossy().into_owned(),
            },
            &resolver,
        )
        .expect("prepared fixture");
        assert_ne!(prepared.image.artifact_path, source.to_string_lossy());
        let result = recognize_drawing_profile(
            RecognizeDrawingProfileRequest {
                image: prepared.image,
                view: SketchView::Top,
                region: DrawingRegion {
                    left_px: 80.0,
                    top_px: 70.0,
                    width_px: 480.0,
                    height_px: 310.0,
                    rotation_degrees: 0.0,
                },
                rectification: None,
                calibration: DrawingScaleCalibration {
                    start_px: [120.0, 100.0],
                    end_px: [520.0, 100.0],
                    distance_mm: 80.0,
                },
                excluded_regions: Vec::new(),
                tolerance_mm: 0.5,
            },
            &resolver,
        )
        .expect("recognized centerline geometry");
        let outer = result
            .profiles
            .iter()
            .find(|profile| profile.role == DrawingProfileRole::Outer && profile.closed)
            .expect("closed plate outline");
        let outer_primitive = result
            .primitives
            .iter()
            .find(|primitive| primitive_id(primitive) == outer.primitive_ids[0])
            .expect("outer primitive");
        let [width, height] = primitive_extent(outer_primitive);
        assert!((width - 80.0).abs() < 1.0, "plate width={width}");
        assert!((height - 50.0).abs() < 1.0, "plate height={height}");
        let circle = result
            .primitives
            .iter()
            .find_map(|primitive| match primitive {
                DrawingPrimitive::Circle {
                    radius_mm,
                    residual_mm,
                    ..
                } => Some((*radius_mm, *residual_mm)),
                _ => None,
            })
            .expect("analytic hole, not two ink boundaries");
        assert!((circle.0 - 5.0).abs() < 0.5, "hole radius={}", circle.0);
        assert!(circle.1 <= 0.5, "circle residual={}", circle.1);
        assert!(result
            .profiles
            .iter()
            .any(|profile| profile.role == DrawingProfileRole::Hole
                && profile.parent_id.as_deref() == Some(&outer.id)));
        assert!(result.evidence_id.len() >= 32);
        assert!(Path::new(&result.evidence.image.artifact_path).is_file());
        assert!(result
            .transformed_preview_data_url
            .starts_with("data:image/png;base64,"));
        let mut edited_primitives = result.primitives.clone();
        if let Some(DrawingPrimitive::Circle { radius_mm, .. }) = edited_primitives
            .iter_mut()
            .find(|primitive| matches!(primitive, DrawingPrimitive::Circle { .. }))
        {
            *radius_mm = 5.1;
        }
        edited_primitives.push(DrawingPrimitive::Line {
            id: "unselected-open-line".into(),
            start_mm: [120.0, 120.0],
            end_mm: [130.0, 120.0],
            residual_mm: 0.0,
        });
        let reviewed = review_drawing_profile(
            ReviewDrawingProfileRequest {
                evidence_id: result.evidence_id,
                primitives: edited_primitives.clone(),
            },
            &resolver,
        )
        .expect("authoritative review of edited image geometry");
        assert!(reviewed.profiles.iter().any(|profile| {
            profile.role == DrawingProfileRole::Outer
                && profile.primitive_ids.iter().all(|id| {
                    edited_primitives
                        .iter()
                        .any(|primitive| primitive_id(primitive) == id)
                })
        }));
        assert!(reviewed
            .profiles
            .iter()
            .any(|profile| profile.role == DrawingProfileRole::Hole));
        assert!(reviewed
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("open or branched")));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn given_skewed_noisy_plate_when_rectified_and_calibrated_then_recovers_metric_outline_and_circle(
    ) {
        let root = std::env::temp_dir().join(format!("ecky-drawing-skew-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("skewed.png");
        plate_fixture(&source, true, true);
        let resolver = Resolver(root.clone());
        let prepared = prepare_drawing_image(
            PrepareDrawingImageRequest {
                source_path: source.to_string_lossy().into_owned(),
            },
            &resolver,
        )
        .expect("prepared skewed plate");
        let result = recognize_drawing_profile(
            RecognizeDrawingProfileRequest {
                image: prepared.image,
                view: SketchView::Top,
                region: DrawingRegion {
                    left_px: 80.0,
                    top_px: 70.0,
                    width_px: 480.0,
                    height_px: 310.0,
                    rotation_degrees: 0.0,
                },
                rectification: Some(DrawingRectification {
                    corners_px: [[112.0, 94.0], [522.0, 112.0], [490.0, 380.0], [94.0, 348.0]],
                    aspect_ratio: 1.6,
                }),
                calibration: DrawingScaleCalibration {
                    start_px: [112.0, 94.0],
                    end_px: [522.0, 112.0],
                    distance_mm: 80.0,
                },
                excluded_regions: Vec::new(),
                tolerance_mm: 1.5,
            },
            &resolver,
        )
        .expect("rectified center geometry");
        let outer = result
            .primitives
            .iter()
            .find_map(|primitive| match primitive {
                DrawingPrimitive::Polyline {
                    points_mm,
                    closed: true,
                    ..
                } => Some(points_mm),
                _ => None,
            })
            .unwrap_or_else(|| {
                panic!(
                    "closed plate centerline; primitives={:?}; diagnostics={:?}",
                    result.primitives, result.diagnostics
                )
            });
        let width = outer
            .iter()
            .map(|point| point[0])
            .fold(f64::NEG_INFINITY, f64::max)
            - outer
                .iter()
                .map(|point| point[0])
                .fold(f64::INFINITY, f64::min);
        let height = outer
            .iter()
            .map(|point| point[1])
            .fold(f64::NEG_INFINITY, f64::max)
            - outer
                .iter()
                .map(|point| point[1])
                .fold(f64::INFINITY, f64::min);
        assert!((width - 80.0).abs() < 1.5, "rectified width={width}");
        assert!((height - 50.0).abs() < 1.5, "rectified height={height}");
        let radius = result
            .primitives
            .iter()
            .find_map(|primitive| match primitive {
                DrawingPrimitive::Circle {
                    radius_mm,
                    residual_mm,
                    ..
                } => {
                    assert!(*residual_mm <= 1.5, "circle residual={residual_mm}");
                    Some(*radius_mm)
                }
                _ => None,
            })
            .expect("full circle retained analytically");
        assert!((radius - 5.0).abs() < 0.7, "rectified hole radius={radius}");
        let top_left = map_point(result.source_to_preview_transform, [112.0, 94.0]).unwrap();
        assert!(top_left[0].abs() < 1.0 && top_left[1].abs() < 1.0);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn given_blank_image_or_invalid_calibration_when_recognized_then_returns_actionable_error() {
        let root =
            std::env::temp_dir().join(format!("ecky-drawing-invalid-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("blank.png");
        GrayImage::from_pixel(240, 180, Luma([255]))
            .save_with_format(&source, ImageFormat::Png)
            .unwrap();
        let resolver = Resolver(root.clone());
        let prepared = prepare_drawing_image(
            PrepareDrawingImageRequest {
                source_path: source.to_string_lossy().into_owned(),
            },
            &resolver,
        )
        .unwrap();
        let base = RecognizeDrawingProfileRequest {
            image: prepared.image,
            view: SketchView::Top,
            region: DrawingRegion {
                left_px: 0.0,
                top_px: 0.0,
                width_px: 240.0,
                height_px: 180.0,
                rotation_degrees: 0.0,
            },
            rectification: None,
            calibration: DrawingScaleCalibration {
                start_px: [20.0, 20.0],
                end_px: [100.0, 20.0],
                distance_mm: 80.0,
            },
            excluded_regions: Vec::new(),
            tolerance_mm: 0.5,
        };
        let blank_error = recognize_drawing_profile(base.clone(), &resolver).unwrap_err();
        assert!(blank_error
            .message
            .contains("No reliable centerline geometry"));
        let mut too_tight = base.clone();
        too_tight.tolerance_mm = 0.02;
        let precision_error = recognize_drawing_profile(too_tight, &resolver).unwrap_err();
        assert!(precision_error
            .message
            .contains("finer than this image's pixel resolution"));
        let mut crossed = base.clone();
        crossed.rectification = Some(DrawingRectification {
            corners_px: [[10.0, 10.0], [100.0, 100.0], [100.0, 10.0], [10.0, 100.0]],
            aspect_ratio: 1.6,
        });
        let corner_error = recognize_drawing_profile(crossed, &resolver).unwrap_err();
        assert!(corner_error.message.contains("non-crossed, non-collinear"));
        let mut invalid = base;
        invalid.calibration.distance_mm = 0.0;
        let calibration_error = recognize_drawing_profile(invalid, &resolver).unwrap_err();
        assert!(calibration_error.message.contains("positive number"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn given_analytic_circle_when_serialized_then_contract_fields_are_camel_case() {
        let value = serde_json::to_value(DrawingPrimitive::Circle {
            id: "hole".into(),
            center_mm: [1.0, 2.0],
            radius_mm: 5.0,
            residual_mm: 0.1,
        })
        .unwrap();
        assert_eq!(value["kind"], "circle");
        assert_eq!(value["centerMm"], serde_json::json!([1.0, 2.0]));
        assert!(value.get("center_mm").is_none());
    }

    fn primitive_id(primitive: &DrawingPrimitive) -> &str {
        match primitive {
            DrawingPrimitive::Line { id, .. }
            | DrawingPrimitive::Circle { id, .. }
            | DrawingPrimitive::Polyline { id, .. } => id,
        }
    }

    fn primitive_extent(primitive: &DrawingPrimitive) -> [f64; 2] {
        let points = match primitive {
            DrawingPrimitive::Polyline { points_mm, .. } => points_mm.as_slice(),
            _ => panic!("outer profile must be polygonal"),
        };
        let min_x = points
            .iter()
            .map(|point| point[0])
            .fold(f64::INFINITY, f64::min);
        let max_x = points
            .iter()
            .map(|point| point[0])
            .fold(f64::NEG_INFINITY, f64::max);
        let min_y = points
            .iter()
            .map(|point| point[1])
            .fold(f64::INFINITY, f64::min);
        let max_y = points
            .iter()
            .map(|point| point[1])
            .fold(f64::NEG_INFINITY, f64::max);
        [max_x - min_x, max_y - min_y]
    }
}
