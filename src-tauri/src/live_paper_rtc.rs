//! Continuous H.264 WebRTC capture for the live paper controller.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Weak};
use std::time::{Duration, Instant};

use axum::extract::{DefaultBodyLimit, Path, Query};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use image::{DynamicImage, RgbImage};
use openh264::decoder::Decoder;
use openh264::formats::YUVSource;
use serde::Deserialize;
use tokio::sync::{mpsc, Mutex};
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::{MediaEngine, MIME_TYPE_H264};
use webrtc::api::APIBuilder;
use webrtc::interceptor::registry::Registry;
use webrtc::media::io::sample_builder::SampleBuilder;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication;
use webrtc::rtp::codecs::h264::H264Packet;
use webrtc::rtp::extension::video_orientation_extension::{
    VideoOrientationExtension, VideoRotation,
};
use webrtc::rtp_transceiver::rtp_codec::{
    RTCRtpCodecCapability, RTCRtpCodecParameters, RTCRtpHeaderExtensionCapability, RTPCodecType,
};
use webrtc::track::track_remote::TrackRemote;
use webrtc::util::marshal::Unmarshal;

use crate::services::live_paper::hub;

const MAX_SDP_BYTES: usize = 128 * 1024;
const MAX_SAMPLE_BYTES: usize = 2 * 1024 * 1024;
const MAX_FRAME_PIXELS: usize = 1920 * 1080;
const VIDEO_ORIENTATION_URI: &str = "urn:3gpp:video-orientation";
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
static NEGOTIATION: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
static ACTIVE: LazyLock<Mutex<Option<ActivePeer>>> = LazyLock::new(|| Mutex::new(None));

type Task = Pin<Box<dyn Future<Output = ()> + Send>>;
type FrameSink = Arc<dyn Fn(DynamicImage) -> Task + Send + Sync>;
type StatusSink = Arc<dyn Fn(&'static str, Option<String>) -> Task + Send + Sync>;

/// TLS and DTLS dependencies enable multiple Rustls providers. Select the
/// existing TLS server's AWS-LC provider explicitly, preserving any host choice.
pub fn initialize_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

struct ActivePeer {
    session_id: String,
    peer: Arc<RTCPeerConnection>,
    cancelled: Arc<AtomicBool>,
}

#[derive(Deserialize)]
struct Offer {
    token: String,
    sdp: String,
}

#[derive(Deserialize)]
struct Authorization {
    token: String,
}

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        message.into(),
    )
        .into_response()
}

/// The same TLS listener serves pairing, signaling, and the phone camera page.
pub fn router() -> Router {
    Router::new()
        .route(
            "/live-paper",
            get(|| async { Html(include_str!("../assets/live_paper_client.html")) }),
        )
        .route(
            "/capture-assets/live_paper_client.mjs",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../assets/live_paper_client.mjs"),
                )
            }),
        )
        .route("/api/live-paper/{session_id}/offer", post(offer))
        .route("/api/live-paper/{session_id}/template.pdf", get(template))
        .route("/api/live-paper/{session_id}/qr.svg", get(pairing_qr))
        .layer(DefaultBodyLimit::max(MAX_SDP_BYTES))
}

async fn pairing_qr(Path(session_id): Path<String>, Query(auth): Query<Authorization>) -> Response {
    if !hub().authorize(&session_id, &auth.token) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Live-paper pairing expired. Open a new link from Ecky.",
        );
    }
    let Some(session) = hub()
        .current()
        .filter(|session| session.session_id == session_id)
    else {
        return error(StatusCode::NOT_FOUND, "Start live paper in Ecky first.");
    };
    match qrcode::QrCode::new(session.phone_url.as_bytes()) {
        Ok(code) => (
            [
                (header::CONTENT_TYPE, "image/svg+xml"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            code.render::<qrcode::render::svg::Color>()
                .min_dimensions(240, 240)
                .build(),
        )
            .into_response(),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not create the live-paper pairing code.",
        ),
    }
}

async fn template(Path(session_id): Path<String>, Query(auth): Query<Authorization>) -> Response {
    if !hub().authorize(&session_id, &auth.token) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Live-paper pairing expired. Open a new link from Ecky.",
        );
    }
    let Some(session) = hub()
        .current()
        .filter(|session| session.session_id == session_id)
    else {
        return error(StatusCode::NOT_FOUND, "Paper session was replaced.");
    };
    match crate::services::live_paper_vision::template_pdf_for_view(&session.view) {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, "application/pdf"),
                (
                    header::CONTENT_DISPOSITION,
                    "inline; filename=\"ecky-live-paper-a4.pdf\"",
                ),
            ],
            bytes,
        )
            .into_response(),
        Err(message) => error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}

/// Close media immediately when Rust stops or replaces its owning paper session.
pub async fn close_session(session_id: &str) {
    let active = {
        let mut slot = ACTIVE.lock().await;
        if slot
            .as_ref()
            .is_some_and(|active| active.session_id == session_id)
        {
            slot.take()
        } else {
            None
        }
    };
    if let Some(active) = active {
        active.cancelled.store(true, Ordering::Release);
        let _ = active.peer.close().await;
    }
}

async fn offer(Path(session_id): Path<String>, Json(request): Json<Offer>) -> Response {
    if !hub().authorize(&session_id, &request.token) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Live-paper pairing expired. Open a new link from Ecky.",
        );
    }
    let Ok(_negotiation) = NEGOTIATION.try_lock() else {
        return error(
            StatusCode::CONFLICT,
            "Another camera is connecting. Try again after it finishes.",
        );
    };
    if request.sdp.len() > MAX_SDP_BYTES {
        return error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "Camera offer exceeds the signaling limit.",
        );
    }
    let remote = match RTCSessionDescription::offer(request.sdp) {
        Ok(description) => description,
        Err(_) => {
            return error(
                StatusCode::BAD_REQUEST,
                "Camera sent an invalid WebRTC offer.",
            )
        }
    };
    close_session(&session_id).await;
    let cancelled = Arc::new(AtomicBool::new(false));
    let frame_session = session_id.clone();
    let frame_token = request.token.clone();
    let frame_cancelled = cancelled.clone();
    let sink: FrameSink = Arc::new(move |image| {
        let session_id = frame_session.clone();
        let token = frame_token.clone();
        let cancelled = frame_cancelled.clone();
        Box::pin(async move {
            if !cancelled.load(Ordering::Acquire) && hub().authorize(&session_id, &token) {
                let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
                let _ = hub().ingest_frame(&session_id, sequence, image).await;
            }
        })
    });
    let state_session = session_id.clone();
    let state_cancelled = cancelled.clone();
    let status: StatusSink = Arc::new(move |state, detail| {
        let session_id = state_session.clone();
        let cancelled = state_cancelled.clone();
        Box::pin(async move {
            if !cancelled.load(Ordering::Acquire) {
                hub().set_transport_state(&session_id, state, detail).await;
            }
        })
    });
    let peer = match new_video_peer(sink, status, cancelled.clone()).await {
        Ok(peer) => peer,
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not initialize the local H.264 WebRTC receiver.",
            )
        }
    };
    *ACTIVE.lock().await = Some(ActivePeer {
        session_id: session_id.clone(),
        peer: peer.clone(),
        cancelled,
    });
    hub()
        .set_transport_state(&session_id, "connecting", None)
        .await;
    let answer = async {
        peer.set_remote_description(remote)
            .await
            .map_err(|e| e.to_string())?;
        let answer = peer.create_answer(None).await.map_err(|e| e.to_string())?;
        let mut complete = peer.gathering_complete_promise().await;
        peer.set_local_description(answer)
            .await
            .map_err(|e| e.to_string())?;
        tokio::time::timeout(Duration::from_secs(12), complete.recv())
            .await
            .map_err(|_| "Local network discovery timed out".to_string())?;
        peer.local_description()
            .await
            .ok_or_else(|| "Local network answer unavailable".to_string())
    }
    .await;
    match answer {
        Ok(answer) if hub().authorize(&session_id, &request.token) => Json(answer).into_response(),
        result => {
            close_session(&session_id).await;
            let message = match result {
                Err(_) => "Could not negotiate H.264 live video. Use Safari or Chrome on the same local network and start again.",
                Ok(_) => "Paper session stopped while the camera was connecting. Open a new link from Ecky.",
            };
            hub()
                .set_transport_state(&session_id, "error", Some(message.into()))
                .await;
            error(StatusCode::BAD_REQUEST, message)
        }
    }
}

fn h264_media_engine() -> Result<MediaEngine, String> {
    let mut engine = MediaEngine::default();
    engine
        .register_header_extension(
            RTCRtpHeaderExtensionCapability {
                uri: VIDEO_ORIENTATION_URI.into(),
            },
            RTPCodecType::Video,
            None,
        )
        .map_err(|e| e.to_string())?;
    for (payload_type, profile) in [(102, "42001f"), (104, "42e01f")] {
        engine.register_codec(RTCRtpCodecParameters {
            capability: RTCRtpCodecCapability {
                mime_type: MIME_TYPE_H264.into(), clock_rate: 90_000,
                sdp_fmtp_line: format!("level-asymmetry-allowed=1;packetization-mode=1;profile-level-id={profile}"),
                ..Default::default()
            }, payload_type, ..Default::default()
        }, RTPCodecType::Video).map_err(|e| e.to_string())?;
    }
    Ok(engine)
}

async fn new_video_peer(
    sink: FrameSink,
    status: StatusSink,
    cancelled: Arc<AtomicBool>,
) -> Result<Arc<RTCPeerConnection>, String> {
    initialize_crypto_provider();
    let mut media = h264_media_engine()?;
    let interceptors =
        register_default_interceptors(Registry::new(), &mut media).map_err(|e| e.to_string())?;
    let api = APIBuilder::new()
        .with_media_engine(media)
        .with_interceptor_registry(interceptors)
        .build();
    let peer = Arc::new(
        api.new_peer_connection(RTCConfiguration::default())
            .await
            .map_err(|e| e.to_string())?,
    );
    let state_status = status.clone();
    peer.on_peer_connection_state_change(Box::new(move |state| {
        let status = state_status.clone();
        Box::pin(async move {
            let state = match state {
                RTCPeerConnectionState::Connected => "streaming",
                RTCPeerConnectionState::Disconnected => "disconnected",
                RTCPeerConnectionState::Failed => "error",
                RTCPeerConnectionState::Closed => "disconnected",
                _ => "connecting",
            };
            status(state, (state == "error").then(|| "Live video connection failed. Check the local network and restart the camera.".into())).await;
        })
    }));
    let weak_peer = Arc::downgrade(&peer);
    let claimed = Arc::new(AtomicBool::new(false));
    peer.on_track(Box::new(move |track, _, _| {
        let sink = sink.clone();
        let status = status.clone();
        let cancelled = cancelled.clone();
        let peer = weak_peer.clone();
        let claimed = claimed.clone();
        Box::pin(async move {
            if claimed.swap(true, Ordering::AcqRel) {
                return;
            }
            if !track
                .codec()
                .capability
                .mime_type
                .eq_ignore_ascii_case(MIME_TYPE_H264)
            {
                status(
                    "error",
                    Some(
                        "Camera must send H.264 video. Reopen this link in Safari or Chrome."
                            .into(),
                    ),
                )
                .await;
                return;
            }
            receive_h264(track, peer, sink, status, cancelled).await;
        })
    }));
    Ok(peer)
}

fn contains_idr(sample: &[u8]) -> bool {
    sample
        .windows(4)
        .any(|bytes| bytes[0..3] == [0, 0, 1] && bytes[3] & 0x1f == 5)
}

struct EncodedFrame {
    data: Vec<u8>,
    orientation: VideoOrientationExtension,
}

#[derive(Default)]
struct FrameOrientations {
    pending: VecDeque<(u32, VideoOrientationExtension)>,
    current: VideoOrientationExtension,
}

impl FrameOrientations {
    fn observe(&mut self, timestamp: u32, mut payload: &[u8]) {
        if payload.len() != 1 {
            return;
        }
        let Ok(orientation) = VideoOrientationExtension::unmarshal(&mut payload) else {
            return;
        };
        if let Some((_, saved)) = self
            .pending
            .iter_mut()
            .find(|(frame, _)| *frame == timestamp)
        {
            *saved = orientation;
        } else {
            // The jitter buffer retains at most 64 packets; bound metadata independently.
            if self.pending.len() == 64 {
                self.pending.pop_front();
            }
            self.pending.push_back((timestamp, orientation));
        }
    }

    fn take(&mut self, timestamp: u32) -> VideoOrientationExtension {
        if let Some(index) = self
            .pending
            .iter()
            .position(|(frame, _)| *frame == timestamp)
        {
            if let Some((_, orientation)) = self.pending.remove(index) {
                self.current = orientation;
            }
        }
        self.current
    }
}

fn orient_frame(frame: RgbImage, orientation: VideoOrientationExtension) -> DynamicImage {
    let frame = DynamicImage::ImageRgb8(frame);
    let frame = if orientation.flip {
        frame.fliph()
    } else {
        frame
    };
    // CVO specifies the clockwise correction, independently of the camera direction.
    match orientation.rotation {
        VideoRotation::Degree0 => frame,
        VideoRotation::Degree90 => frame.rotate90(),
        VideoRotation::Degree180 => frame.rotate180(),
        VideoRotation::Degree270 => frame.rotate270(),
    }
}

async fn receive_h264(
    track: Arc<TrackRemote>,
    peer: Weak<RTCPeerConnection>,
    sink: FrameSink,
    status: StatusSink,
    cancelled: Arc<AtomicBool>,
) {
    // Encoded interframes depend on prior frames. Overflow drops until the next IDR,
    // then resumes with a fresh decoder instead of feeding a corrupted history.
    let (encoded_tx, mut encoded_rx) = mpsc::channel::<EncodedFrame>(8);
    let resync = Arc::new(AtomicBool::new(false));
    let decode_resync = resync.clone();
    let decode_cancelled = cancelled.clone();
    let decode_status = status.clone();
    let clock = Instant::now();
    let last_decoded_ms = Arc::new(AtomicU64::new(0));
    let decode_last_ms = last_decoded_ms.clone();
    let stalled = Arc::new(AtomicBool::new(false));
    let decode_stalled = stalled.clone();
    let runtime = tokio::runtime::Handle::current();
    let decoder = tokio::task::spawn_blocking(move || {
        let mut decoder = match Decoder::new() {
            Ok(decoder) => decoder,
            Err(_) => {
                runtime.block_on(decode_status(
                    "error",
                    Some("The bundled H.264 decoder could not start.".into()),
                ));
                return;
            }
        };
        let mut waiting_for_idr = false;
        while let Some(sample) = encoded_rx.blocking_recv() {
            if decode_cancelled.load(Ordering::Acquire) {
                break;
            }
            if decode_resync.swap(false, Ordering::AcqRel) {
                waiting_for_idr = true;
            }
            if waiting_for_idr {
                if !contains_idr(&sample.data) {
                    continue;
                }
                let Ok(fresh) = Decoder::new() else { break };
                decoder = fresh;
                waiting_for_idr = false;
            }
            match decoder.decode(&sample.data) {
                Ok(Some(decoded)) => {
                    let (width, height) = decoded.dimensions();
                    if width == 0
                        || height == 0
                        || width.max(height) > 1920
                        || width.min(height) > 1080
                        || width.saturating_mul(height) > MAX_FRAME_PIXELS
                    {
                        runtime.block_on(decode_status("error", Some("Camera video exceeds 1920×1080. Restart with a lower camera resolution.".into())));
                        break;
                    }
                    let mut rgb = vec![0u8; width * height * 3];
                    decoded.write_rgb8(&mut rgb);
                    if let Some(frame) = RgbImage::from_raw(width as u32, height as u32, rgb) {
                        decode_last_ms.store(clock.elapsed().as_millis() as u64, Ordering::Release);
                        if decode_stalled.swap(false, Ordering::AcqRel) {
                            runtime.block_on(decode_status("streaming", None));
                        }
                        runtime.block_on(sink(orient_frame(frame, sample.orientation)));
                    }
                }
                Ok(None) => (),
                Err(_) => {
                    waiting_for_idr = true;
                    decode_resync.store(true, Ordering::Release);
                }
            }
        }
    });
    let mut samples = SampleBuilder::new(64, H264Packet::default(), 90_000)
        .with_max_time_delay(Duration::from_millis(250));
    let orientation_extension_id = track
        .params()
        .header_extensions
        .iter()
        .find(|extension| extension.uri == VIDEO_ORIENTATION_URI)
        .and_then(|extension| u8::try_from(extension.id).ok());
    let mut orientations = FrameOrientations::default();
    let mut keyframes = tokio::time::interval(Duration::from_secs(2));
    loop {
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        tokio::select! {
            _ = keyframes.tick() => {
                let idle_ms = (clock.elapsed().as_millis() as u64).saturating_sub(last_decoded_ms.load(Ordering::Acquire));
                if idle_ms >= 6_000 && !stalled.swap(true, Ordering::AcqRel) {
                    status("error", Some("Camera video paused. Keep the Safari page visible and camera unobstructed, or restart the camera. Accepted geometry is preserved.".into())).await;
                }
                let Some(peer) = peer.upgrade() else { break };
                let _ = peer.write_rtcp(&[Box::new(PictureLossIndication { sender_ssrc: 0, media_ssrc: track.ssrc() })]).await;
            }
            packet = track.read_rtp() => {
                let Ok((packet, _)) = packet else { break };
                if packet.payload.len() > 64 * 1024 { continue; }
                if let Some(payload) = orientation_extension_id.and_then(|id| packet.header.get_extension(id)) {
                    orientations.observe(packet.header.timestamp, &payload);
                }
                samples.push(packet);
                while let Some(sample) = samples.pop() {
                    let orientation = orientations.take(sample.packet_timestamp);
                    if sample.prev_dropped_packets > 0 { resync.store(true, Ordering::Release); }
                    if sample.data.len() > MAX_SAMPLE_BYTES { resync.store(true, Ordering::Release); continue; }
                    if encoded_tx.try_send(EncodedFrame { data: sample.data.to_vec(), orientation }).is_err() { resync.store(true, Ordering::Release); }
                }
            }
        }
    }
    drop(encoded_tx);
    let _ = decoder.await;
    if !cancelled.load(Ordering::Acquire) {
        status("disconnected", Some("Camera stream ended. Accepted geometry is preserved; restart the phone camera to resume.".into())).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    #[tokio::test]
    async fn given_unpaired_phone_when_offering_video_then_reject_before_creating_peer() {
        let response = router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/live-paper/unpaired/offer")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"token":"wrong","sdp":"not-an-offer"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn given_interframe_or_keyframe_when_resynchronizing_then_only_accept_idr() {
        assert!(!contains_idr(&[0, 0, 0, 1, 0x41, 3, 4]));
        assert!(contains_idr(&[0, 0, 0, 1, 0x65, 3, 4]));
    }

    async fn test_sender(
        receiver: &Arc<RTCPeerConnection>,
    ) -> (
        Arc<RTCPeerConnection>,
        Arc<webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample>,
    ) {
        use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;
        use webrtc::track::track_local::TrackLocal;
        let mut media = h264_media_engine().unwrap();
        media
            .register_header_extension(
                webrtc::rtp_transceiver::rtp_codec::RTCRtpHeaderExtensionCapability {
                    uri: "urn:3gpp:video-orientation".into(),
                },
                RTPCodecType::Video,
                None,
            )
            .unwrap();
        let api = APIBuilder::new().with_media_engine(media).build();
        let sender = Arc::new(
            api.new_peer_connection(RTCConfiguration::default())
                .await
                .unwrap(),
        );
        let track = Arc::new(TrackLocalStaticSample::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_H264.into(),
                clock_rate: 90_000,
                sdp_fmtp_line:
                    "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f".into(),
                ..Default::default()
            },
            "paper".into(),
            "test-video".into(),
        ));
        sender
            .add_track(track.clone() as Arc<dyn TrackLocal + Send + Sync>)
            .await
            .unwrap();
        let mut gathered = sender.gathering_complete_promise().await;
        sender
            .set_local_description(sender.create_offer(None).await.unwrap())
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), gathered.recv())
            .await
            .unwrap();
        receiver
            .set_remote_description(sender.local_description().await.unwrap())
            .await
            .unwrap();
        let mut gathered = receiver.gathering_complete_promise().await;
        receiver
            .set_local_description(receiver.create_answer(None).await.unwrap())
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), gathered.recv())
            .await
            .unwrap();
        sender
            .set_remote_description(receiver.local_description().await.unwrap())
            .await
            .unwrap();
        (sender, track)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn given_continuous_h264_rtc_video_when_received_then_decode_real_pen_pixels() {
        use openh264::encoder::Encoder;
        use openh264::formats::{RgbSliceU8, YUVBuffer};
        use webrtc::media::Sample;

        let (frames_tx, mut frames_rx) = mpsc::channel(4);
        let sink: FrameSink = Arc::new(move |frame| {
            let tx = frames_tx.clone();
            Box::pin(async move {
                let _ = tx.try_send(frame);
            })
        });
        let receiver = new_video_peer(
            sink,
            Arc::new(|_, _| Box::pin(async {})),
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .unwrap();
        let (sender, track) = test_sender(&receiver).await;

        let encoded = tokio::task::spawn_blocking(|| {
            let mut rgb = vec![255; 96 * 96 * 3];
            for y in 45..51 {
                for x in 16..80 {
                    for color in 0..3 {
                        rgb[(y * 96 + x) * 3 + color] = 0;
                    }
                }
            }
            let yuv = YUVBuffer::from_rgb_source(RgbSliceU8::new(&rgb, (96, 96)));
            Encoder::new().unwrap().encode(&yuv).unwrap().to_vec()
        })
        .await
        .unwrap();
        let send_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(50));
            for _ in 0..100 {
                interval.tick().await;
                track
                    .write_sample(&Sample {
                        data: encoded.clone().into(),
                        duration: Duration::from_millis(50),
                        ..Default::default()
                    })
                    .await
                    .unwrap();
            }
        });
        let received = tokio::time::timeout(Duration::from_secs(8), async {
            let first = frames_rx.recv().await.unwrap();
            let second = frames_rx.recv().await.unwrap();
            (first, second)
        })
        .await;
        send_task.abort();
        sender.close().await.unwrap();
        receiver.close().await.unwrap();
        let (first, second) = received.expect("continuous RTC must yield multiple decoded frames");
        assert_eq!(first.width(), 96);
        let gray = second.to_luma8();
        assert!(
            gray.get_pixel(48, 48)[0] < 50,
            "pen center must stay dark after media encoding and decoding"
        );
        assert!(
            gray.get_pixel(48, 16)[0] > 200,
            "paper must stay light after media encoding and decoding"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn given_mobile_rtc_orientation_when_phone_rotates_then_pixels_stay_upright_per_frame() {
        use openh264::encoder::Encoder;
        use openh264::formats::{RgbSliceU8, YUVBuffer};
        use webrtc::media::Sample;
        use webrtc::rtp::extension::video_orientation_extension::{
            VideoOrientationExtension, VideoRotation,
        };
        let (frames_tx, mut frames_rx) = mpsc::unbounded_channel();
        let sink: FrameSink = Arc::new(move |frame| {
            let frames_tx = frames_tx.clone();
            Box::pin(async move {
                let _ = frames_tx.send(frame);
            })
        });
        let receiver = new_video_peer(
            sink,
            Arc::new(|_, _| Box::pin(async {})),
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .unwrap();
        let (sender, track) = test_sender(&receiver).await;
        assert!(
            receiver
                .local_description()
                .await
                .unwrap()
                .sdp
                .contains("urn:3gpp:video-orientation"),
            "Receiver must negotiate mobile rotation metadata"
        );
        let encoded = tokio::task::spawn_blocking(|| {
            let mut image = RgbImage::from_pixel(96, 64, image::Rgb([255; 3]));
            for y in 8..24 {
                for x in 16..40 {
                    image.put_pixel(x, y, image::Rgb([0; 3]));
                }
            }
            let yuv = YUVBuffer::from_rgb_source(RgbSliceU8::new(image.as_raw(), (96, 64)));
            Encoder::new().unwrap().encode(&yuv).unwrap().to_vec()
        })
        .await
        .unwrap();
        let (orientation_tx, orientation_rx) =
            tokio::sync::watch::channel(VideoOrientationExtension::default());
        let send_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(60));
            loop {
                interval.tick().await;
                let orientation = *orientation_rx.borrow();
                if track
                    .sample_writer()
                    .with_video_orientation(orientation)
                    .write_sample(&Sample {
                        data: encoded.clone().into(),
                        duration: Duration::from_millis(60),
                        ..Default::default()
                    })
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
        let result = tokio::time::timeout(Duration::from_secs(12), async {
            for (rotation, flip, dimensions, dark) in [
                (VideoRotation::Degree0, false, (96, 64), (28, 16)),
                (VideoRotation::Degree90, false, (64, 96), (47, 28)),
                (VideoRotation::Degree180, false, (96, 64), (67, 47)),
                (VideoRotation::Degree270, false, (64, 96), (16, 67)),
                (VideoRotation::Degree90, true, (64, 96), (47, 67)),
                (VideoRotation::Degree0, false, (96, 64), (28, 16)),
            ] {
                orientation_tx
                    .send(VideoOrientationExtension {
                        rotation,
                        flip,
                        ..Default::default()
                    })
                    .unwrap();
                loop {
                    let frame = frames_rx.recv().await.unwrap().to_luma8();
                    if frame.dimensions() == dimensions && frame.get_pixel(dark.0, dark.1)[0] < 60 {
                        assert!(
                            frame.get_pixel(dimensions.0 - 1 - dark.0, dimensions.1 - 1 - dark.1)
                                [0]
                                > 200
                        );
                        break;
                    }
                }
            }
        })
        .await;
        send_task.abort();
        sender.close().await.unwrap();
        receiver.close().await.unwrap();
        result.expect(
            "Quarter-turns and flip must transform their own decoded frame without reconnecting",
        );
    }

    #[test]
    fn given_reordered_orientation_metadata_when_frames_pop_then_keep_each_frames_rotation() {
        let mut orientations = FrameOrientations::default();
        orientations.observe(101, &[2]);
        orientations.observe(100, &[1]);
        assert_eq!(orientations.take(100).rotation, VideoRotation::Degree90);
        assert_eq!(orientations.take(101).rotation, VideoRotation::Degree180);
        orientations.observe(102, &[]);
        orientations.observe(102, &[1, 2]);
        assert_eq!(orientations.take(102).rotation, VideoRotation::Degree180);
        orientations.observe(103, &[0]);
        assert_eq!(orientations.take(103).rotation, VideoRotation::Degree0);
        for timestamp in 104..400 {
            orientations.observe(timestamp, &[1]);
        }
        assert_eq!(orientations.pending.len(), 64);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 3)]
    async fn given_real_rtc_paper_frames_when_recognized_then_accept_geometry_and_preserve_on_marker_loss(
    ) {
        use crate::services::live_paper::LivePaperPrimitive;
        use openh264::encoder::Encoder;
        use openh264::formats::{RgbSliceU8, YUVBuffer};
        use webrtc::media::Sample;
        let session = hub().start("https://localhost:40000");
        let id = session.session_id.clone();
        let sink: FrameSink = Arc::new(move |image| {
            let id = id.clone();
            Box::pin(async move {
                let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
                hub().ingest_frame(&id, sequence, image).await.unwrap();
            })
        });
        let receiver = new_video_peer(
            sink,
            Arc::new(|_, _| Box::pin(async {})),
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .unwrap();
        let (sender, track) = test_sender(&receiver).await;
        let encoded = tokio::task::spawn_blocking(|| {
            [
                include_bytes!("../tests/fixtures/live-paper/primitives.png").as_slice(),
                include_bytes!("../tests/fixtures/live-paper/occlusion.png").as_slice(),
                include_bytes!("../tests/fixtures/live-paper/marker-loss.png").as_slice(),
            ]
            .into_iter()
            .map(|bytes| {
                let rgb = image::load_from_memory(bytes).unwrap().to_rgb8();
                let yuv = YUVBuffer::from_rgb_source(RgbSliceU8::new(
                    rgb.as_raw(),
                    (rgb.width() as usize, rgb.height() as usize),
                ));
                Encoder::new().unwrap().encode(&yuv).unwrap().to_vec()
            })
            .collect::<Vec<_>>()
        })
        .await
        .unwrap();
        let (scene_tx, scene_rx) = tokio::sync::watch::channel(0usize);
        let send_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(150));
            loop {
                interval.tick().await;
                let scene = *scene_rx.borrow();
                if track
                    .write_sample(&Sample {
                        data: encoded[scene].clone().into(),
                        duration: Duration::from_millis(150),
                        ..Default::default()
                    })
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
        let mut events = hub().subscribe();
        let acceptance = tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let snapshot = events.recv().await.unwrap();
                if snapshot.accepted.len() >= 4 {
                    break snapshot;
                }
            }
        })
        .await;
        if acceptance.is_err() {
            send_task.abort();
            let _ = sender.close().await;
            let _ = receiver.close().await;
            panic!(
                "RTC-encoded paper must produce accepted geometry: {:?}",
                hub().current().map(|snapshot| (
                    snapshot.state,
                    snapshot.detail,
                    snapshot.sequence,
                    snapshot.accepted.len(),
                    snapshot.pending.len(),
                    snapshot.diagnostics
                ))
            );
        }
        let accepted_snapshot = acceptance.unwrap();
        let accepted_sequence = accepted_snapshot.sequence;
        let accepted = accepted_snapshot.accepted;
        assert!(accepted
            .iter()
            .any(|p| matches!(p, LivePaperPrimitive::Point { .. })));
        assert!(accepted
            .iter()
            .any(|p| matches!(p, LivePaperPrimitive::Line { .. })));
        assert!(accepted
            .iter()
            .any(|p| matches!(p, LivePaperPrimitive::Circle { .. })));
        assert!(accepted
            .iter()
            .any(|p| matches!(p, LivePaperPrimitive::Polyline { closed: true, .. })));
        scene_tx.send(1).unwrap();
        let occluded = tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let snapshot = events.recv().await.unwrap();
                if snapshot.state == "streaming"
                    && snapshot.sequence > accepted_sequence
                    && snapshot
                        .diagnostics
                        .iter()
                        .any(|detail| detail.contains("occlusion"))
                {
                    break snapshot;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(
            occluded.accepted, accepted,
            "occlusion must preserve accepted identities"
        );
        scene_tx.send(2).unwrap();
        let lost = tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let snapshot = events.recv().await.unwrap();
                if snapshot.state == "markerLost" {
                    break snapshot;
                }
            }
        })
        .await;
        send_task.abort();
        sender.close().await.unwrap();
        receiver.close().await.unwrap();
        hub().stop(&session.session_id).unwrap();
        assert_eq!(
            lost.unwrap().accepted,
            accepted,
            "marker loss must preserve accepted identities"
        );
    }
}
