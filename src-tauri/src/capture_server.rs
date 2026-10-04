use crate::capture_reconstruction::{
    AppleObjectCaptureProvider, ProgressCallback, ReconstructionCancellation, ReconstructionInput,
    ReconstructionProvider,
};
use crate::contracts::{
    AppError, AppResult, CaptureFrameManifest, CaptureFrameManifestEntry, CaptureFrameMetrics,
    CapturePairRequest, CaptureServerAssessment, CaptureSessionInfo, CaptureSessionState,
};
use crate::models::{AppState, PathResolver};
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair,
    SanType,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io;
use std::net::{IpAddr, Ipv4Addr, UdpSocket};
use std::path::{Path as FsPath, PathBuf};
use std::sync::Arc;
use tokio::net::TcpListener;

const MAX_FRAME_BYTES: usize = 24 * 1024 * 1024;
const MIN_RECONSTRUCTION_FRAMES: usize = 20;
const CAPTURE_CLIENT_HTML: &str = include_str!("../assets/capture_client.html");
const CAPTURE_METRICS_JS: &str = include_str!("../assets/capture_metrics.mjs");

#[derive(Clone)]
struct CaptureHttpState {
    app: AppState,
    root: PathBuf,
    tool_cache_dir: PathBuf,
}

#[derive(Clone)]
struct TrustHttpState {
    ca_der: Arc<Vec<u8>>,
    app: AppState,
    trust_url: String,
}

struct CaptureCa {
    certificate: Certificate,
    key_pair: KeyPair,
    pem: String,
    der: Vec<u8>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FrameUploadResponse {
    frame: CaptureFrameManifestEntry,
    created: bool,
}

fn raw_error(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        message.into(),
    )
        .into_response()
}

fn header_text<'a>(headers: &'a HeaderMap, name: &'static str) -> Result<&'a str, Response> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| raw_error(StatusCode::BAD_REQUEST, format!("Missing `{name}` header.")))
}

fn safe_frame_id(frame_id: &str) -> bool {
    (1..=96).contains(&frame_id.len())
        && frame_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn visual_signature(image: &image::DynamicImage) -> Vec<u8> {
    image
        .resize_exact(8, 8, image::imageops::FilterType::Triangle)
        .to_luma8()
        .into_raw()
}

fn feature_overlap(left: &[u8], right: &[u8]) -> f32 {
    if left.is_empty() || left.len() != right.len() {
        return 0.0;
    }
    let difference = left
        .iter()
        .zip(right)
        .map(|(left, right)| (*left as f32 - *right as f32).abs())
        .sum::<f32>()
        / left.len() as f32;
    (1.0 - difference / 255.0).clamp(0.0, 1.0)
}

fn frame_assessment(accepted_count: usize, overlap: f32) -> CaptureServerAssessment {
    let guidance = if accepted_count < MIN_RECONSTRUCTION_FRAMES {
        format!("Collect at least {MIN_RECONSTRUCTION_FRAMES} frames, then inspect preview")
    } else {
        "Preview available; add photos after inspection if needed".to_string()
    };
    CaptureServerAssessment {
        feature_overlap: overlap,
        coverage_percent: 0,
        guidance,
        missing_view: None,
    }
}

fn local_lan_ip() -> IpAddr {
    UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
        .and_then(|socket| {
            socket.connect((Ipv4Addr::new(1, 1, 1, 1), 80))?;
            socket.local_addr()
        })
        .map(|address| address.ip())
        .ok()
        .filter(|ip| !ip.is_loopback() && !ip.is_unspecified())
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST))
}

fn rcgen_io(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

fn certificate_der_from_pem(pem: &str) -> io::Result<Vec<u8>> {
    let body: String = pem
        .lines()
        .filter(|line| !line.starts_with("---"))
        .collect();
    base64::engine::general_purpose::STANDARD
        .decode(body)
        .map_err(rcgen_io)
}

fn capture_ca(certificate_root: &FsPath) -> io::Result<CaptureCa> {
    let cert_path = certificate_root.join("capture-ca.pem");
    let key_path = certificate_root.join("capture-ca-key.pem");
    if cert_path.exists() && key_path.exists() {
        let cert_pem = std::fs::read_to_string(cert_path)?;
        let key_pem = std::fs::read_to_string(key_path)?;
        let key = KeyPair::from_pem(&key_pem).map_err(rcgen_io)?;
        let params = CertificateParams::from_ca_cert_pem(&cert_pem).map_err(rcgen_io)?;
        let certificate = params.self_signed(&key).map_err(rcgen_io)?;
        return Ok(CaptureCa {
            certificate,
            key_pair: key,
            der: certificate_der_from_pem(&cert_pem)?,
            pem: cert_pem,
        });
    }

    std::fs::create_dir_all(certificate_root)?;
    let mut params = CertificateParams::default();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, "Ecky Local Capture CA");
    name.push(DnType::OrganizationName, "Ecky");
    params.distinguished_name = name;
    let key_pair = KeyPair::generate().map_err(rcgen_io)?;
    let certificate = params.self_signed(&key_pair).map_err(rcgen_io)?;
    std::fs::write(&cert_path, certificate.pem())?;
    std::fs::write(&key_path, key_pair.serialize_pem())?;
    Ok(CaptureCa {
        pem: certificate.pem(),
        der: certificate.der().to_vec(),
        certificate,
        key_pair,
    })
}

fn capture_leaf(ca: &CaptureCa, ip: IpAddr) -> io::Result<(Vec<u8>, Vec<u8>)> {
    let mut params = CertificateParams::new(Vec::new()).map_err(rcgen_io)?;
    params.subject_alt_names.push(SanType::IpAddress(ip));
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, "Ecky Capture");
    params.distinguished_name = name;
    let key_pair = KeyPair::generate().map_err(rcgen_io)?;
    let certificate = params
        .signed_by(&key_pair, &ca.certificate, &ca.key_pair)
        .map_err(rcgen_io)?;
    let chain_pem = format!("{}\n{}", certificate.pem(), ca.pem);
    Ok((
        chain_pem.into_bytes(),
        key_pair.serialize_pem().into_bytes(),
    ))
}

async fn trust_page() -> Html<&'static str> {
    Html(
        r#"<!doctype html>
<meta name=viewport content='width=device-width,initial-scale=1'>
<title>Ecky Capture Trust</title>
<style>
html{color-scheme:dark;background:#090c12;color:#e7e1d6;font:16px ui-monospace,monospace}
body{max-width:640px;margin:40px auto;padding:16px}
li{margin:18px 0;line-height:1.5}
a{display:inline-flex;padding:12px;border:1px solid #c89a58;color:#c89a58;text-decoration:none}
.required{padding:14px;border:1px solid #c89a58}.required strong{display:block;color:#c89a58;margin-bottom:8px}
code{overflow-wrap:anywhere;color:#fff}
</style>
<h1>Trust Ecky on this iPhone</h1>
<ol>
  <li><a href='/ecky-capture-ca.cer'>Download Ecky certificate</a></li>
  <li>Install downloaded profile:<br><code>Settings &gt; General &gt; VPN &amp; Device Management &gt; Downloaded Profile &gt; Install</code></li>
  <li class=required><strong>Required: enable Full Trust</strong><code>Settings &gt; General &gt; About &gt; Certificate Trust Settings &gt; Ecky Local Capture CA</code><p>Safari will still show a privacy warning until Full Trust is enabled.</p></li>
</ol>
<p>Complete these steps once. Then open the Capture QR.</p>"#,
    )
}

async fn trust_certificate(State(server): State<TrustHttpState>) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/x-x509-ca-cert"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=ecky-capture-ca.cer",
            ),
        ],
        server.ca_der.as_ref().clone(),
    )
        .into_response()
}

fn qr_response(value: &str) -> Response {
    match qrcode::QrCode::new(value.as_bytes()) {
        Ok(code) => {
            let svg = code
                .render::<qrcode::render::svg::Color>()
                .min_dimensions(256, 256)
                .dark_color(qrcode::render::svg::Color("#090c12"))
                .light_color(qrcode::render::svg::Color("#e7e1d6"))
                .build();
            (
                [(header::CONTENT_TYPE, "image/svg+xml; charset=utf-8")],
                svg,
            )
                .into_response()
        }
        Err(error) => raw_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("QR generation failed: {error}"),
        ),
    }
}

async fn trust_qr(State(server): State<TrustHttpState>) -> Response {
    qr_response(&server.trust_url)
}

async fn bootstrap_pairing_qr(
    State(server): State<TrustHttpState>,
    Path(token): Path<String>,
) -> Response {
    match session_or_error(server.app.get_capture_session(&token).await) {
        Ok(session) => qr_response(&session.pairing_url),
        Err(response) => response,
    }
}

fn session_or_error(session: Option<CaptureSessionInfo>) -> Result<CaptureSessionInfo, Response> {
    session.ok_or_else(|| {
        raw_error(
            StatusCode::UNAUTHORIZED,
            "Capture token is invalid, expired, or revoked.",
        )
    })
}

async fn capture_page(
    State(server): State<CaptureHttpState>,
    Path(token): Path<String>,
) -> Response {
    match session_or_error(server.app.get_capture_session(&token).await) {
        Ok(_) => Html(CAPTURE_CLIENT_HTML).into_response(),
        Err(response) => response,
    }
}

async fn capture_metrics_script() -> Response {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        CAPTURE_METRICS_JS,
    )
        .into_response()
}

async fn pairing_qr(State(server): State<CaptureHttpState>, Path(token): Path<String>) -> Response {
    let session = match session_or_error(server.app.get_capture_session(&token).await) {
        Ok(session) => session,
        Err(response) => return response,
    };
    qr_response(&session.pairing_url)
}

async fn session_status(
    State(server): State<CaptureHttpState>,
    Path(token): Path<String>,
) -> Response {
    match session_or_error(server.app.get_capture_session(&token).await) {
        Ok(session) => Json(session).into_response(),
        Err(response) => response,
    }
}

async fn pair_session(
    State(server): State<CaptureHttpState>,
    Path(token): Path<String>,
    body: Bytes,
) -> Response {
    let request = if body.is_empty() {
        CapturePairRequest::default()
    } else {
        match serde_json::from_slice::<CapturePairRequest>(&body) {
            Ok(request) => request,
            Err(error) => {
                return raw_error(
                    StatusCode::BAD_REQUEST,
                    format!("Pairing payload invalid: {error}"),
                )
            }
        }
    };
    match server
        .app
        .pair_capture_session(&token, request.protocol_version, request.capabilities)
        .await
    {
        Ok(session) => Json(session).into_response(),
        Err(error) => raw_error(StatusCode::UNAUTHORIZED, error.message),
    }
}

async fn frame_manifest(
    State(server): State<CaptureHttpState>,
    Path(token): Path<String>,
) -> Response {
    let Some(session) = server.app.get_capture_session(&token).await else {
        return raw_error(
            StatusCode::UNAUTHORIZED,
            "Capture token is invalid, expired, or revoked.",
        );
    };
    match server.app.capture_manifest(&token).await {
        Ok(frames) => Json(CaptureFrameManifest {
            session_id: session.session_id,
            frames,
        })
        .into_response(),
        Err(error) => raw_error(StatusCode::BAD_REQUEST, error.message),
    }
}

fn parse_u64_header(headers: &HeaderMap, name: &'static str) -> Result<u64, Response> {
    header_text(headers, name)?
        .parse()
        .map_err(|_| raw_error(StatusCode::BAD_REQUEST, format!("Invalid `{name}` header.")))
}

fn parse_u32_header(headers: &HeaderMap, name: &'static str) -> Result<u32, Response> {
    header_text(headers, name)?
        .parse()
        .map_err(|_| raw_error(StatusCode::BAD_REQUEST, format!("Invalid `{name}` header.")))
}

fn parse_f32_header(headers: &HeaderMap, name: &'static str) -> Result<f32, Response> {
    let value = header_text(headers, name)?
        .parse::<f32>()
        .map_err(|_| raw_error(StatusCode::BAD_REQUEST, format!("Invalid `{name}` header.")))?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(raw_error(
            StatusCode::BAD_REQUEST,
            format!("Invalid `{name}` header."),
        ))
    }
}

fn parse_optional_f32_array<const N: usize>(
    headers: &HeaderMap,
    name: &'static str,
) -> Result<Option<[f32; N]>, Response> {
    let Some(raw) = headers.get(name) else {
        return Ok(None);
    };
    let raw = raw
        .to_str()
        .map_err(|_| raw_error(StatusCode::BAD_REQUEST, format!("Invalid `{name}` header.")))?;
    let values = raw
        .split(',')
        .map(str::trim)
        .map(str::parse::<f32>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| raw_error(StatusCode::BAD_REQUEST, format!("Invalid `{name}` header.")))?;
    let values: [f32; N] = values.try_into().map_err(|_| {
        raw_error(
            StatusCode::BAD_REQUEST,
            format!("`{name}` requires {N} numbers."),
        )
    })?;
    if values.iter().all(|value| value.is_finite()) {
        Ok(Some(values))
    } else {
        Err(raw_error(
            StatusCode::BAD_REQUEST,
            format!("Invalid `{name}` header."),
        ))
    }
}

async fn persist_manifest(
    root: &FsPath,
    session_id: &str,
    frames: &[CaptureFrameManifestEntry],
) -> io::Result<()> {
    let session_root = root.join(session_id);
    tokio::fs::create_dir_all(&session_root).await?;
    let destination = session_root.join("manifest.json");
    let temporary = session_root.join("manifest.json.tmp");
    let bytes = serde_json::to_vec_pretty(&CaptureFrameManifest {
        session_id: session_id.to_string(),
        frames: frames.to_vec(),
    })?;
    tokio::fs::write(&temporary, bytes).await?;
    tokio::fs::rename(temporary, destination).await
}

async fn upload_frame(
    State(server): State<CaptureHttpState>,
    Path((token, frame_id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let session = match session_or_error(server.app.get_capture_session(&token).await) {
        Ok(session) => session,
        Err(response) => return response,
    };
    if !safe_frame_id(&frame_id) {
        return raw_error(
            StatusCode::BAD_REQUEST,
            "Frame id contains unsupported characters.",
        );
    }
    let mime_type = match header_text(&headers, "content-type") {
        Ok("image/jpeg") => "image/jpeg",
        Ok("image/png") => "image/png",
        Ok(other) => {
            return raw_error(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                format!("Unsupported image MIME `{other}`."),
            )
        }
        Err(response) => return response,
    };
    if body.is_empty() {
        return raw_error(StatusCode::BAD_REQUEST, "Image body is empty.");
    }
    let expected_digest = match header_text(&headers, "x-content-digest") {
        Ok(value) => value.to_ascii_lowercase(),
        Err(response) => return response,
    };
    let actual_digest = format!("{:x}", Sha256::digest(&body));
    if expected_digest != actual_digest {
        return raw_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("Digest mismatch: expected `{expected_digest}`, received `{actual_digest}`."),
        );
    }
    let decoded = match image::load_from_memory(&body) {
        Ok(image) => image,
        Err(error) => {
            return raw_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("Image decode failed: {error}"),
            )
        }
    };
    let signature = visual_signature(&decoded);
    let existing_frames = match server.app.capture_manifest(&token).await {
        Ok(frames) => frames,
        Err(error) => return raw_error(StatusCode::UNAUTHORIZED, error.message),
    };
    if let Some(existing) = existing_frames
        .iter()
        .find(|frame| frame.content_digest == actual_digest)
    {
        return Json(FrameUploadResponse {
            frame: existing.clone(),
            created: false,
        })
        .into_response();
    }
    let overlap = existing_frames
        .iter()
        .map(|frame| feature_overlap(&signature, &frame.visual_signature))
        .fold(0.0_f32, f32::max);
    if !existing_frames.is_empty() && overlap > 0.985 {
        return raw_error(
            StatusCode::CONFLICT,
            format!("Duplicate view rejected: visual overlap {overlap:.3}. Move to a new angle."),
        );
    }
    if !existing_frames.is_empty() && overlap < 0.08 {
        return raw_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("Frame overlap too low ({overlap:.3}). Return toward the previous view."),
        );
    }
    let assessment = frame_assessment(existing_frames.len() + 1, overlap);
    let declared_width = match parse_u32_header(&headers, "x-image-width") {
        Ok(value) => value,
        Err(response) => return response,
    };
    let declared_height = match parse_u32_header(&headers, "x-image-height") {
        Ok(value) => value,
        Err(response) => return response,
    };
    if decoded.width() != declared_width || decoded.height() != declared_height {
        return raw_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!(
                "Decoded image is {}x{}, metadata declares {}x{}.",
                decoded.width(),
                decoded.height(),
                declared_width,
                declared_height
            ),
        );
    }
    let captured_at = match parse_u64_header(&headers, "x-captured-at") {
        Ok(value) => value,
        Err(response) => return response,
    };
    let client_metrics = match (
        parse_f32_header(&headers, "x-luminance"),
        parse_f32_header(&headers, "x-sharpness"),
        parse_f32_header(&headers, "x-subject-coverage"),
        parse_f32_header(&headers, "x-motion"),
    ) {
        (Ok(luminance), Ok(sharpness), Ok(subject_coverage), Ok(motion)) => {
            Some(CaptureFrameMetrics {
                luminance,
                sharpness,
                subject_coverage,
                motion,
            })
        }
        (Err(response), _, _, _)
        | (_, Err(response), _, _)
        | (_, _, Err(response), _)
        | (_, _, _, Err(response)) => return response,
    };
    let camera_intrinsics = match parse_optional_f32_array::<9>(&headers, "x-camera-intrinsics") {
        Ok(value) => value,
        Err(response) => return response,
    };
    let camera_transform = match parse_optional_f32_array::<16>(&headers, "x-camera-transform") {
        Ok(value) => value,
        Err(response) => return response,
    };
    let depth_digest = headers
        .get("x-depth-digest")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let extension = if mime_type == "image/png" {
        "png"
    } else {
        "jpg"
    };
    let relative_path = format!("source/{actual_digest}.{extension}");
    let session_root = server.root.join(&session.session_id);
    let source_root = session_root.join("source");
    if let Err(error) = tokio::fs::create_dir_all(&source_root).await {
        return raw_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Capture storage create failed: {error}"),
        );
    }
    let image_path = source_root.join(format!("{actual_digest}.{extension}"));
    if !image_path.exists() {
        if let Err(error) = tokio::fs::write(&image_path, &body).await {
            return raw_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Frame write failed: {error}"),
            );
        }
    }
    let frame = CaptureFrameManifestEntry {
        frame_id,
        content_digest: actual_digest,
        captured_at,
        mime_type: mime_type.to_string(),
        width: decoded.width(),
        height: decoded.height(),
        image_path: relative_path,
        client_metrics,
        camera_intrinsics,
        camera_transform,
        depth_digest,
        visual_signature: signature,
        server_assessment: assessment,
    };
    match server.app.add_capture_frame(&token, frame).await {
        Ok((frame, created)) => {
            let frames = server
                .app
                .capture_manifest(&token)
                .await
                .unwrap_or_default();
            if let Err(error) = persist_manifest(&server.root, &session.session_id, &frames).await {
                return raw_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Manifest write failed: {error}"),
                );
            }
            Json(FrameUploadResponse { frame, created }).into_response()
        }
        Err(error) => raw_error(StatusCode::CONFLICT, error.message),
    }
}

async fn finish_session(
    State(server): State<CaptureHttpState>,
    Path(token): Path<String>,
) -> Response {
    match begin_reconstruction(server.app, &token, server.root, server.tool_cache_dir).await {
        Ok(session) => Json(session).into_response(),
        Err(error) => raw_error(StatusCode::UNPROCESSABLE_ENTITY, error.message),
    }
}

pub async fn begin_reconstruction(
    state: AppState,
    token: &str,
    root: PathBuf,
    tool_cache_dir: PathBuf,
) -> AppResult<CaptureSessionInfo> {
    if matches!(
        state
            .get_capture_session(token)
            .await
            .map(|session| session.state),
        Some(CaptureSessionState::Reconstructing)
    ) {
        return Err(AppError::conflict(
            "Capture reconstruction is already running.",
        ));
    }
    let frames = state.capture_manifest(token).await?;
    if frames.len() < MIN_RECONSTRUCTION_FRAMES {
        return Err(AppError::validation(format!(
            "Reconstruction requires at least {MIN_RECONSTRUCTION_FRAMES} accepted frames; session has {} frames. Preview it, then add photos if mesh quality is insufficient.",
            frames.len()
        )));
    }
    let session = state
        .set_capture_session_state(&token, CaptureSessionState::Reconstructing)
        .await?;
    state.set_capture_reconstruction_progress(token, 0.0).await;
    let provider = AppleObjectCaptureProvider;
    if let Err(error) = provider.availability(&tool_cache_dir) {
        state
            .fail_capture_reconstruction(token, error.message.clone())
            .await?;
        return Err(error);
    }
    let input = ReconstructionInput {
        session_id: session.session_id.clone(),
        manifest: CaptureFrameManifest {
            session_id: session.session_id.clone(),
            frames,
        },
        source_dir: root.join(&session.session_id).join("source"),
        output_dir: root.join(&session.session_id).join("reconstruction"),
        tool_cache_dir,
    };
    let cancellation = ReconstructionCancellation::default();
    state
        .register_capture_reconstruction(token, cancellation.clone())
        .await;
    let progress_token = token.to_string();
    let progress_state = state.clone();
    let progress: ProgressCallback = Arc::new(move |value| {
        let state = progress_state.clone();
        let token = progress_token.clone();
        tokio::spawn(async move {
            state
                .set_capture_reconstruction_progress(&token, value)
                .await
        });
    });
    let job_token = token.to_string();
    tokio::spawn(async move {
        match provider.reconstruct(&input, progress, cancellation).await {
            Ok(result) => {
                let _ = state
                    .complete_capture_reconstruction(&job_token, result.preview)
                    .await;
            }
            Err(error) => {
                if let Err(persistence_error) = state
                    .fail_capture_reconstruction(&job_token, error.message)
                    .await
                {
                    state.push_log(format!(
                        "[CAPTURE] Failed to persist reconstruction error: {}",
                        persistence_error.message
                    ));
                }
            }
        }
        state.clear_capture_reconstruction(&job_token).await;
    });
    Ok(session)
}

pub fn router(state: AppState, root: PathBuf) -> Router {
    let tool_cache_dir = root.parent().unwrap_or(&root).join("capture-tools");
    Router::new()
        .route(
            "/capture-assets/capture_metrics.mjs",
            get(capture_metrics_script),
        )
        .route("/capture/{token}", get(capture_page))
        .route("/capture/{token}/qr.svg", get(pairing_qr))
        .route("/api/capture/{token}", get(session_status))
        .route("/api/capture/{token}/pair", post(pair_session))
        .route("/api/capture/{token}/frames", get(frame_manifest))
        .route("/api/capture/{token}/frames/{frame_id}", post(upload_frame))
        .route("/api/capture/{token}/finish", post(finish_session))
        .layer(DefaultBodyLimit::max(MAX_FRAME_BYTES))
        .with_state(CaptureHttpState {
            app: state,
            root,
            tool_cache_dir,
        })
}

pub async fn serve(state: AppState, app: Arc<dyn PathResolver + Send + Sync>) -> io::Result<()> {
    let ip = local_lan_ip();
    let certificate_root = app.app_data_dir().join("capture-certificates");
    let ca = capture_ca(&certificate_root)?;
    let ca_der = ca.der.clone();
    let (certificate_pem, key_pem) = capture_leaf(&ca, ip)?;
    let tls = axum_server::tls_rustls::RustlsConfig::from_pem(certificate_pem, key_pem)
        .await
        .map_err(io::Error::other)?;

    let tls_listener = std::net::TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    tls_listener.set_nonblocking(true)?;
    let tls_port = tls_listener.local_addr()?.port();
    let trust_listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
    let trust_port = trust_listener.local_addr()?.port();
    let base_url = format!("https://{ip}:{tls_port}");
    let trust_url = format!("http://{ip}:{trust_port}/trust");
    *state.capture_server_url.lock().unwrap() = Some(base_url.clone());
    *state.capture_trust_url.lock().unwrap() = Some(trust_url.clone());
    state.push_log(format!("[CAPTURE] Listening on {base_url}"));
    let mdns = mdns_sd::ServiceDaemon::new().map_err(io::Error::other)?;
    let service_type = "_ecky-capture._tcp.local.";
    let properties = [("protocol", "1"), ("trust", trust_url.as_str())];
    let service = mdns_sd::ServiceInfo::new(
        service_type,
        "Ecky Capture",
        "ecky-capture.local.",
        ip,
        tls_port,
        &properties[..],
    )
    .map_err(io::Error::other)?;
    mdns.register(service).map_err(io::Error::other)?;
    let root = app.app_data_dir().join("captures");
    tokio::fs::create_dir_all(&root).await?;
    let trust_router = Router::new()
        .route("/trust", get(trust_page))
        .route("/trust/qr.svg", get(trust_qr))
        .route("/capture/{token}/qr.svg", get(bootstrap_pairing_qr))
        .route("/ecky-capture-ca.cer", get(trust_certificate))
        .with_state(TrustHttpState {
            ca_der: Arc::new(ca_der),
            app: state.clone(),
            trust_url: trust_url.clone(),
        });
    let capture_server = axum_server::from_tcp_rustls(tls_listener, tls)
        .serve(router(state.clone(), root).into_make_service());
    let trust_server = axum::serve(trust_listener, trust_router);
    let result = tokio::select! {
        result = capture_server => result.map_err(io::Error::other),
        result = trust_server => result.map_err(io::Error::other),
    };
    *state.capture_server_url.lock().unwrap() = None;
    *state.capture_trust_url.lock().unwrap() = None;
    let _ = mdns.shutdown();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{
        Config, EngineKind, GeometryBackend, McpConfig, SourceLanguage, VoiceConfig,
    };
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use image::{DynamicImage, ImageFormat, RgbImage};
    use std::io::Cursor;
    use tower::ServiceExt;

    fn test_state() -> AppState {
        let config = Config {
            engines: vec![],
            selected_engine_id: String::new(),
            freecad_cmd: String::new(),
            cad_text_font_path: String::new(),
            freecad_library_roots: vec![],
            assets: vec![],
            microwave: None,
            voice: VoiceConfig::default(),
            mcp: McpConfig::default(),
            fem_compute: crate::contracts::FemComputeConfig::default(),
            has_seen_onboarding: true,
            connection_type: None,
            provider_models: crate::contracts::ProviderModels::default(),
            jev_classifier: Default::default(),
            default_engine_kind: EngineKind::EckyIrV0,
            default_source_language: SourceLanguage::EckyIrV0,
            default_geometry_backend: GeometryBackend::EckyRust,
            max_generation_attempts: 1,
            max_verify_attempts: 0,
            projects_root: None,
        };
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::capture_runs::ensure_schema(&conn).expect("capture schema");
        let state = AppState::new(config, None, conn);
        *state.capture_server_url.lock().unwrap() = Some("http://192.0.2.1:44000".into());
        *state.capture_trust_url.lock().unwrap() = Some("http://192.0.2.1:44001/trust".into());
        state
    }

    fn jpeg() -> Vec<u8> {
        let image = RgbImage::from_pixel(2, 2, image::Rgb([32, 64, 96]));
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(image)
            .write_to(&mut bytes, ImageFormat::Jpeg)
            .unwrap();
        bytes.into_inner()
    }

    fn png() -> Vec<u8> {
        let image = RgbImage::from_pixel(2, 2, image::Rgb([32, 64, 96]));
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(image)
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }

    #[tokio::test]
    async fn trust_page_explains_both_required_ios_settings_steps() {
        let page = trust_page().await;

        assert!(page
            .0
            .contains("Settings &gt; General &gt; VPN &amp; Device Management"));
        assert!(page
            .0
            .contains("Settings &gt; General &gt; About &gt; Certificate Trust Settings"));
        assert!(page.0.contains("Safari will still show a privacy warning"));
    }

    #[test]
    fn feature_overlap_and_batch_guidance_are_deterministic() {
        assert_eq!(feature_overlap(&[10, 20, 30], &[10, 20, 30]), 1.0);
        assert!(feature_overlap(&[0; 64], &[255; 64]) < 0.01);
        let early = frame_assessment(3, 0.7);
        assert_eq!(early.coverage_percent, 0);
        assert!(early.guidance.contains("inspect preview"));
    }

    #[tokio::test]
    async fn reconstruction_readiness_policy_rejects_insufficient_evidence_before_provider() {
        let state = test_state();
        let session = state
            .start_capture_session(3600, "thread-test".into(), None)
            .await
            .unwrap();
        let root =
            std::env::temp_dir().join(format!("ecky-capture-readiness-{}", uuid::Uuid::new_v4()));
        let error = begin_reconstruction(
            state.clone(),
            &session.pairing_token,
            root.clone(),
            root.join("tools"),
        )
        .await
        .unwrap_err();
        assert!(error.message.contains("20 accepted frames"));
        assert_eq!(
            state
                .get_capture_session(&session.pairing_token)
                .await
                .unwrap()
                .state,
            CaptureSessionState::Pairing
        );
    }

    #[tokio::test]
    async fn unknown_token_cannot_read_capture_api() {
        let root = std::env::temp_dir().join(format!("ecky-capture-test-{}", uuid::Uuid::new_v4()));
        let response = router(test_state(), root)
            .oneshot(
                Request::get("/api/capture/not-a-session")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], b"Capture token is invalid, expired, or revoked.");
    }

    #[tokio::test]
    async fn frame_upload_validates_digest_and_is_idempotent() {
        let state = test_state();
        let session = state
            .start_capture_session(3600, "thread-test".into(), None)
            .await
            .unwrap();
        let root = std::env::temp_dir().join(format!("ecky-capture-test-{}", uuid::Uuid::new_v4()));
        let app = router(state.clone(), root.clone());
        let bytes = jpeg();
        let digest = format!("{:x}", Sha256::digest(&bytes));
        let uri = format!("/api/capture/{}/frames/frame-1", session.pairing_token);
        let upload = || {
            Request::post(&uri)
                .header("content-type", "image/jpeg")
                .header("x-content-digest", &digest)
                .header("x-captured-at", "123")
                .header("x-image-width", "2")
                .header("x-image-height", "2")
                .header("x-luminance", "96")
                .header("x-sharpness", "20")
                .header("x-subject-coverage", "0.4")
                .header("x-motion", "2")
                .header("x-camera-intrinsics", "1,0,0,0,1,0,0,0,1")
                .header("x-camera-transform", "1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1")
                .header("x-depth-digest", "depth-abc")
                .body(Body::from(bytes.clone()))
                .unwrap()
        };

        let first = app.clone().oneshot(upload()).await.unwrap();
        assert_eq!(first.status(), StatusCode::OK);
        let first_body = to_bytes(first.into_body(), 4096).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&first_body).unwrap()["created"],
            true
        );

        let duplicate = app.clone().oneshot(upload()).await.unwrap();
        let duplicate_body = to_bytes(duplicate.into_body(), 4096).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&duplicate_body).unwrap()["created"],
            false
        );
        let stored = state
            .capture_manifest(&session.pairing_token)
            .await
            .unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(
            stored[0].client_metrics.as_ref().unwrap().subject_coverage,
            0.4
        );
        assert_eq!(
            stored[0].camera_intrinsics,
            Some([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0])
        );
        assert_eq!(
            stored[0].camera_transform,
            Some([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0])
        );
        assert_eq!(stored[0].depth_digest.as_deref(), Some("depth-abc"));

        let mismatch = app
            .oneshot(
                Request::post(format!(
                    "/api/capture/{}/frames/frame-2",
                    session.pairing_token
                ))
                .header("content-type", "image/jpeg")
                .header("x-content-digest", "wrong")
                .header("x-captured-at", "124")
                .header("x-image-width", "2")
                .header("x-image-height", "2")
                .header("x-luminance", "96")
                .header("x-sharpness", "20")
                .header("x-subject-coverage", "0.4")
                .header("x-motion", "2")
                .body(Body::from(bytes))
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(mismatch.status(), StatusCode::UNPROCESSABLE_ENTITY);

        let visual_duplicate = png();
        let visual_duplicate_digest = format!("{:x}", Sha256::digest(&visual_duplicate));
        let duplicate_view = router(state.clone(), root.clone())
            .oneshot(
                Request::post(format!(
                    "/api/capture/{}/frames/frame-visual-duplicate",
                    session.pairing_token
                ))
                .header("content-type", "image/png")
                .header("x-content-digest", visual_duplicate_digest)
                .header("x-captured-at", "125")
                .header("x-image-width", "2")
                .header("x-image-height", "2")
                .header("x-luminance", "96")
                .header("x-sharpness", "20")
                .header("x-subject-coverage", "0.4")
                .header("x-motion", "2")
                .body(Body::from(visual_duplicate))
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(duplicate_view.status(), StatusCode::CONFLICT);
        let duplicate_reason = to_bytes(duplicate_view.into_body(), 4096).await.unwrap();
        assert!(String::from_utf8_lossy(&duplicate_reason).contains("Duplicate view rejected"));

        std::fs::remove_dir_all(root).unwrap();
    }
}
