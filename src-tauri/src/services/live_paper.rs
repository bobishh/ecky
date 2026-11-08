//! Authoritative transient paper capture and stable sketch geometry.

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LivePaperPrimitive {
    #[specta(rename_all = "camelCase")]
    Point {
        id: String,
        position_mm: [f64; 2],
        residual_mm: f64,
    },
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

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LivePaperSnapshot {
    pub session_id: String,
    pub scope_key: String,
    pub view: crate::contracts::SketchView,
    pub phone_url: String,
    pub phone_qr_data_url: String,
    pub template_url: String,
    pub template_data_url: String,
    pub trust_url: Option<String>,
    pub state: String,
    pub detail: Option<String>,
    pub sequence: u64,
    pub width_mm: f64,
    pub height_mm: f64,
    pub preview_data_url: Option<String>,
    pub camera_preview_data_url: Option<String>,
    pub preview_kind: String,
    pub accepted: Vec<LivePaperPrimitive>,
    pub pending: Vec<LivePaperPrimitive>,
    pub diagnostics: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_restored(actual: &[LivePaperPrimitive], expected: &[LivePaperPrimitive]) {
        fn close(a: &serde_json::Value, b: &serde_json::Value) {
            match (a, b) {
                (serde_json::Value::Number(a), serde_json::Value::Number(b)) => {
                    assert!((a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 1e-9)
                }
                (serde_json::Value::Array(a), serde_json::Value::Array(b)) => {
                    assert_eq!(a.len(), b.len());
                    for (a, b) in a.iter().zip(b) {
                        close(a, b);
                    }
                }
                (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
                    assert_eq!(a.len(), b.len());
                    for (key, value) in a {
                        close(value, b.get(key).unwrap());
                    }
                }
                _ => assert_eq!(a, b),
            }
        }
        close(
            &serde_json::to_value(actual).unwrap(),
            &serde_json::to_value(expected).unwrap(),
        );
    }

    fn line(id: &str, x: f64) -> LivePaperPrimitive {
        LivePaperPrimitive::Line {
            id: id.into(),
            start_mm: [x, 10.0],
            end_mm: [x + 30.0, 10.0],
            residual_mm: 0.1,
        }
    }

    async fn processed(
        events: &mut tokio::sync::broadcast::Receiver<LivePaperSnapshot>,
        sequence: u64,
    ) -> LivePaperSnapshot {
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let snapshot = events.recv().await.unwrap();
                if snapshot.sequence >= sequence {
                    return snapshot;
                }
            }
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn given_camera_without_markers_when_first_frame_arrives_then_show_pixels_before_recognition(
    ) {
        let service = Arc::new(LivePaperHub::default());
        let session = service.start("https://127.0.0.1:40000");
        let mut events = service.subscribe();
        service
            .ingest_frame(
                &session.session_id,
                1,
                image::DynamicImage::new_rgb8(800, 600),
            )
            .await
            .unwrap();
        let first = tokio::time::timeout(std::time::Duration::from_secs(10), events.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            first.sequence, 0,
            "Preview must arrive before marker recognition finishes"
        );
        assert_eq!(first.preview_kind, "camera");
        assert!(first
            .camera_preview_data_url
            .as_ref()
            .is_some_and(|url| url.starts_with("data:image/jpeg;base64,")));
        let lost = processed(&mut events, 1).await;
        assert_eq!(lost.state, "markerLost");
        assert!(lost.camera_preview_data_url.is_some());
        assert!(lost.accepted.is_empty());
    }

    #[tokio::test]
    async fn given_distinct_view_templates_when_camera_view_changes_then_gate_wrong_paper_and_preserve_each_view(
    ) {
        use crate::contracts::SketchView;
        let service = Arc::new(LivePaperHub::default());
        let directory =
            std::env::temp_dir().join(format!("ecky-paper-views-{}", uuid::Uuid::new_v4()));
        let path = directory.join("draft.json");
        service.configure_storage(path.clone()).unwrap();
        let session = service.start("https://127.0.0.1:40000");
        let mut events = service.subscribe();
        let front = image::load_from_memory(include_bytes!(
            "../../tests/fixtures/live-paper/primitives.png"
        ))
        .unwrap();
        let top = image::load_from_memory(include_bytes!(
            "../../tests/fixtures/live-paper/primitives-top.png"
        ))
        .unwrap();
        for sequence in 1..=3 {
            service
                .ingest_frame(&session.session_id, sequence, front.clone())
                .await
                .unwrap();
            processed(&mut events, sequence).await;
        }
        let accepted_front = service.current().unwrap().accepted;
        assert_eq!(accepted_front.len(), 4);
        let switched = service
            .set_view(&session.session_id, SketchView::Top)
            .unwrap();
        assert!(switched.accepted.is_empty());
        assert_eq!(switched.phone_url, session.phone_url);
        assert_ne!(switched.template_data_url, session.template_data_url);
        service
            .ingest_frame(&session.session_id, 4, front)
            .await
            .unwrap();
        let mismatch = processed(&mut events, 4).await;
        assert_eq!(mismatch.state, "templateMismatch");
        assert!(mismatch.accepted.is_empty());
        assert!(mismatch.camera_preview_data_url.is_some());
        for sequence in 5..=7 {
            service
                .ingest_frame(&session.session_id, sequence, top.clone())
                .await
                .unwrap();
            processed(&mut events, sequence).await;
        }
        let accepted_top = service.current().unwrap().accepted;
        assert_eq!(accepted_top.len(), 4);
        assert_eq!(
            service
                .set_view(&session.session_id, SketchView::Front)
                .unwrap()
                .accepted,
            accepted_front
        );
        assert_eq!(
            service
                .set_view(&session.session_id, SketchView::Top)
                .unwrap()
                .accepted,
            accepted_top
        );
        let restored = LivePaperHub::default();
        restored.configure_storage(path).unwrap();
        let saved = restored.current().unwrap();
        assert_eq!(saved.view, SketchView::Top);
        assert_restored(&saved.accepted, &accepted_top);
        assert_restored(
            &restored
                .set_view(&saved.session_id, SketchView::Front)
                .unwrap()
                .accepted,
            &accepted_front,
        );
        restored.stop(&saved.session_id).unwrap();
        let reconnect = restored.start_scoped(
            "https://127.0.0.1:40000",
            saved.scope_key.clone(),
            SketchView::Front,
        );
        assert_restored(&reconnect.accepted, &accepted_front);
        assert_ne!(reconnect.session_id, saved.session_id);
        assert_restored(
            &restored
                .set_view(&reconnect.session_id, SketchView::Top)
                .unwrap()
                .accepted,
            &accepted_top,
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn given_stable_frames_when_repeated_stale_or_occluded_then_identity_survives() {
        let mut session = Session::new("https://127.0.0.1:40000");
        session.observe(1, vec![(line("frame-1", 10.0), false)]);
        assert!(session.snapshot.accepted.is_empty());
        session.observe(2, vec![(line("frame-2", 10.1), false)]);
        session.observe(3, vec![(line("frame-3", 9.9), false)]);
        assert_eq!(session.snapshot.accepted.len(), 1);
        let accepted = session.snapshot.accepted[0].clone();
        session.observe(3, vec![(line("stale", 50.0), false)]);
        session.observe(4, vec![]);
        session.observe(5, vec![(line("return", 10.2), false)]);
        assert_eq!(session.snapshot.accepted, vec![accepted]);
    }

    #[test]
    fn given_ambiguous_geometry_when_confirmed_edited_or_rejected_then_review_is_authoritative() {
        let mut session = Session::new("https://127.0.0.1:40000");
        session.observe(1, vec![(line("ambiguous", 10.0), true)]);
        session.observe(2, vec![(line("ambiguous-again", 10.0), true)]);
        assert!(session.snapshot.accepted.is_empty());
        assert_eq!(session.snapshot.pending.len(), 1);
        let id = session.snapshot.pending[0].id().to_owned();
        session.confirm(&id).unwrap();
        session.edit(line(&id, 15.0)).unwrap();
        session.observe(3, vec![(line("original", 10.0), true)]);
        assert_eq!(session.snapshot.accepted.len(), 1);
        session.reject(&id).unwrap();
        session.observe(4, vec![(line("rejected", 10.0), true)]);
        assert!(session.snapshot.accepted.is_empty());
        assert!(session.snapshot.pending.is_empty());
    }

    #[test]
    fn given_stopped_or_replaced_session_when_late_frames_arrive_then_they_cannot_mutate() {
        let service = LivePaperHub::default();
        let old = service.start("https://127.0.0.1:40000");
        let current = service.start("https://127.0.0.1:40000");
        assert!(service.session_matches(&old.session_id).is_err());
        service.stop(&current.session_id).unwrap();
        assert!(service.session_matches(&current.session_id).is_err());
        assert_eq!(service.current().unwrap().state, "stopped");
    }

    #[test]
    fn given_accepted_outline_when_occlusion_splits_it_then_fragments_do_not_duplicate() {
        let mut session = Session::new("https://127.0.0.1:40000");
        for sequence in 1..=3 {
            session.observe(sequence, vec![(line("whole", 10.0), false)]);
        }
        let fragment = LivePaperPrimitive::Line {
            id: "partial".into(),
            start_mm: [15.0, 10.1],
            end_mm: [30.0, 10.0],
            residual_mm: 0.1,
        };
        for sequence in 4..=7 {
            session.observe(sequence, vec![(fragment.clone(), false)]);
        }
        assert_eq!(session.snapshot.accepted.len(), 1);
        assert!(session.snapshot.pending.is_empty());
    }

    #[test]
    fn given_manual_pending_edit_when_new_observation_arrives_then_keep_user_correction() {
        let mut session = Session::new("https://127.0.0.1:40000");
        session.observe(1, vec![(line("ambiguous", 10.0), true)]);
        let id = session.snapshot.pending[0].id().to_owned();
        let edited = line(&id, 15.0);
        session.edit(edited.clone()).unwrap();
        session.observe(2, vec![(line("original", 10.0), true)]);
        assert_eq!(session.snapshot.pending, vec![edited]);
    }

    #[tokio::test]
    async fn given_latest_camera_frames_when_processed_then_auto_accept_and_pause_without_geometry_loss(
    ) {
        let service = Arc::new(LivePaperHub::default());
        let session = service.start("https://127.0.0.1:40000");
        let mut updates = service.subscribe();
        let image = image::load_from_memory(include_bytes!(
            "../../tests/fixtures/live-paper/primitives.png"
        ))
        .unwrap();
        // Before yielding to the worker, newer frames replace the queued frame.
        for sequence in 1..=10 {
            service
                .ingest_frame(&session.session_id, sequence, image.clone())
                .await
                .unwrap();
        }
        let first = processed(&mut updates, 10).await;
        assert_eq!(first.sequence, 10);
        assert_eq!(first.state, "streaming");
        for sequence in 11..=12 {
            service
                .ingest_frame(&session.session_id, sequence, image.clone())
                .await
                .unwrap();
            processed(&mut updates, sequence).await;
        }
        let accepted = service.current().unwrap().accepted;
        assert!(accepted
            .iter()
            .any(|p| matches!(p, LivePaperPrimitive::Circle { .. })));
        service
            .ingest_frame(
                &session.session_id,
                13,
                image::DynamicImage::new_luma8(800, 600),
            )
            .await
            .unwrap();
        let paused = processed(&mut updates, 13).await;
        assert_eq!(paused.state, "markerLost");
        assert_eq!(paused.accepted, accepted);
        service
            .ingest_frame(&session.session_id, 12, image)
            .await
            .unwrap();
        assert_eq!(service.current().unwrap().sequence, 13);
    }

    #[test]
    fn given_unseen_candidates_when_noise_disappears_then_release_capacity_but_keep_edits() {
        let mut session = Session::new("https://127.0.0.1:40000");
        session.observe(
            1,
            vec![(line("noise", 10.0), true), (line("user", 50.0), true)],
        );
        let id = session
            .snapshot
            .pending
            .iter()
            .find(|p| matches!(p,LivePaperPrimitive::Line {start_mm,..} if start_mm[0]>40.0))
            .unwrap()
            .id()
            .to_owned();
        session.edit(line(&id, 55.0)).unwrap();
        for sequence in 2..=50 {
            session.observe(sequence, vec![]);
        }
        assert_eq!(session.snapshot.pending, vec![line(&id, 55.0)]);
    }

    #[test]
    fn given_closed_lines_and_circle_when_prepared_then_join_and_classify_with_analytic_hole() {
        use super::super::profile_authoring::{ProfileLoopRole, ProfileSketchGeometry};
        let mut session = Session::new("https://127.0.0.1:40000");
        let mut geometry = vec![(
            LivePaperPrimitive::Circle {
                id: "hole".into(),
                center_mm: [50.0, 50.0],
                radius_mm: 10.0,
                residual_mm: 0.1,
            },
            false,
        )];
        let corners = [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]];
        for i in 0..4 {
            geometry.push((
                LivePaperPrimitive::Line {
                    id: format!("edge-{i}"),
                    start_mm: corners[i],
                    end_mm: corners[(i + 1) % 4],
                    residual_mm: 0.1,
                },
                false,
            ));
        }
        for sequence in 1..=3 {
            session.observe(sequence, geometry.clone());
        }
        let profile = prepare_profile(&session.snapshot).unwrap();
        assert_eq!(profile.view, crate::contracts::SketchView::Front);
        assert_eq!(profile.sketch_loops.len(), 2);
        let hole = profile
            .sketch_loops
            .iter()
            .find(|p| p.role == ProfileLoopRole::Hole)
            .unwrap();
        assert!(matches!(
            hole.geometry,
            ProfileSketchGeometry::Circle {
                radius_mm: 10.0,
                ..
            }
        ));
        assert!(hole.parent_index.is_some());
    }

    #[tokio::test]
    async fn given_disconnected_camera_when_late_pixels_arrive_then_pause_until_transport_recovers()
    {
        let service = Arc::new(LivePaperHub::default());
        let session = service.start("https://127.0.0.1:40000");
        let image = image::load_from_memory(include_bytes!(
            "../../tests/fixtures/live-paper/primitives.png"
        ))
        .unwrap();
        service
            .ingest_frame(&session.session_id, 1, image.clone())
            .await
            .unwrap();
        service
            .set_transport_state(&session.session_id, "disconnected", None)
            .await;
        assert!(service
            .ingest_frame(&session.session_id, 2, image.clone())
            .await
            .is_err());
        assert_eq!(service.current().unwrap().state, "error");
        assert_eq!(
            service
                .set_view(&session.session_id, crate::contracts::SketchView::Top)
                .unwrap()
                .state,
            "error"
        );
        assert_eq!(
            service
                .set_view(&session.session_id, crate::contracts::SketchView::Front)
                .unwrap()
                .state,
            "error"
        );
        service
            .set_transport_state(&session.session_id, "streaming", None)
            .await;
        let mut events = service.subscribe();
        service
            .ingest_frame(&session.session_id, 3, image)
            .await
            .unwrap();
        let snapshot = processed(&mut events, 3).await;
        assert_eq!(snapshot.state, "streaming");
        assert_eq!(snapshot.sequence, 3);
    }

    #[test]
    fn given_saved_capture_when_restarted_then_restore_editable_ids_and_scope_without_video() {
        let directory =
            std::env::temp_dir().join(format!("ecky-paper-hub-{}", uuid::Uuid::new_v4()));
        let path = directory.join("draft.json");
        let service = LivePaperHub::default();
        service.configure_storage(path.clone()).unwrap();
        let session = service.start_scoped(
            "https://127.0.0.1:40000",
            "model-source-sha".into(),
            crate::contracts::SketchView::Top,
        );
        for sequence in 1..=3 {
            service.finish_frame(
                &session.session_id,
                sequence,
                0,
                Ok(super::super::live_paper_vision::LivePaperObservation {
                    view: crate::contracts::SketchView::Top,
                    preview_data_url: "data:image/png;base64,transient".into(),
                    width_mm: 160.0,
                    height_mm: 220.0,
                    primitives: vec![super::super::live_paper_vision::LivePaperCandidate {
                        primitive: line("observed", 10.0),
                        needs_confirmation: false,
                    }],
                    diagnostics: vec![],
                }),
            );
        }
        let accepted = service.current().unwrap().accepted;
        let restored = LivePaperHub::default();
        restored.configure_storage(path.clone()).unwrap();
        let snapshot = restored.current().unwrap();
        assert_eq!(snapshot.state, "stopped");
        assert_eq!(snapshot.scope_key, "model-source-sha");
        assert_eq!(snapshot.view, crate::contracts::SketchView::Top);
        assert_eq!(snapshot.accepted, accepted);
        assert!(snapshot.preview_data_url.is_none());
        assert!(!restored.authorize(&session.session_id, "old-token"));
        // Failed durable edits leave the authoritative accepted geometry unchanged.
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(restored
            .edit(&snapshot.session_id, line(accepted[0].id(), 20.0))
            .is_err());
        assert_eq!(restored.current().unwrap().accepted, accepted);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn given_invalid_manual_geometry_when_edited_then_reject_without_mutation() {
        let mut session = Session::new("https://127.0.0.1:40000");
        session.observe(1, vec![(line("candidate", 10.0), true)]);
        assert_eq!(session.snapshot.pending.len(), 1);
        let id = session.snapshot.pending[0].id().to_owned();
        let invalid = LivePaperPrimitive::Circle {
            id,
            center_mm: [0.0, 0.0],
            radius_mm: -1.0,
            residual_mm: 0.0,
        };
        assert!(session.edit(invalid).is_err());
        assert_eq!(session.snapshot.pending.len(), 1);
    }
}

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, LazyLock, Mutex};
use tokio::sync::broadcast;

const MATCH_TOLERANCE_MM: f64 = 1.2;
const STABLE_OBSERVATIONS: u32 = 3;
const MAX_ELEMENTS: usize = 256;

impl LivePaperPrimitive {
    pub fn id(&self) -> &str {
        match self {
            Self::Point { id, .. }
            | Self::Line { id, .. }
            | Self::Circle { id, .. }
            | Self::Polyline { id, .. } => id,
        }
    }

    fn set_id(&mut self, value: String) {
        match self {
            Self::Point { id, .. }
            | Self::Line { id, .. }
            | Self::Circle { id, .. }
            | Self::Polyline { id, .. } => *id = value,
        }
    }

    fn valid(&self) -> bool {
        let point = |p: &[f64; 2]| p.iter().all(|n| n.is_finite() && n.abs() <= 1.0e6);
        let residual = |n: f64| n.is_finite() && n >= 0.0;
        match self {
            Self::Point {
                position_mm,
                residual_mm,
                ..
            } => point(position_mm) && residual(*residual_mm),
            Self::Line {
                start_mm,
                end_mm,
                residual_mm,
                ..
            } => {
                point(start_mm)
                    && point(end_mm)
                    && distance(*start_mm, *end_mm) > 0.001
                    && residual(*residual_mm)
            }
            Self::Circle {
                center_mm,
                radius_mm,
                residual_mm,
                ..
            } => {
                point(center_mm)
                    && radius_mm.is_finite()
                    && *radius_mm > 0.0
                    && *radius_mm <= 1.0e6
                    && residual(*residual_mm)
            }
            Self::Polyline {
                points_mm,
                closed,
                residual_mm,
                ..
            } => {
                points_mm.len() >= if *closed { 3 } else { 2 }
                    && points_mm.len() <= 4096
                    && points_mm.iter().all(point)
                    && residual(*residual_mm)
            }
        }
    }

    fn matches(&self, other: &Self) -> bool {
        let tolerance = MATCH_TOLERANCE_MM;
        match (self, other) {
            (Self::Point { position_mm: a, .. }, Self::Point { position_mm: b, .. }) => {
                distance(*a, *b) <= tolerance
            }
            (
                Self::Line {
                    start_mm: a,
                    end_mm: b,
                    ..
                },
                Self::Line {
                    start_mm: c,
                    end_mm: d,
                    ..
                },
            ) => {
                (distance(*a, *c).max(distance(*b, *d))).min(distance(*a, *d).max(distance(*b, *c)))
                    <= tolerance
            }
            (
                Self::Circle {
                    center_mm: a,
                    radius_mm: ar,
                    ..
                },
                Self::Circle {
                    center_mm: b,
                    radius_mm: br,
                    ..
                },
            ) => distance(*a, *b) + (ar - br).abs() <= tolerance,
            (
                Self::Polyline {
                    points_mm: a,
                    closed: ac,
                    ..
                },
                Self::Polyline {
                    points_mm: b,
                    closed: bc,
                    ..
                },
            ) if ac == bc => {
                if !ac
                    && distance(a[0], b[0])
                        .max(distance(a[a.len() - 1], b[b.len() - 1]))
                        .min(distance(a[0], b[b.len() - 1]).max(distance(a[a.len() - 1], b[0])))
                        > tolerance
                {
                    return false;
                }
                path_distance(a, b, *bc).max(path_distance(b, a, *ac)) <= tolerance
            }
            _ => false,
        }
    }
}

fn covers(accepted: &LivePaperPrimitive, observed: &LivePaperPrimitive) -> bool {
    // A partial stroke through an occluder is evidence for its existing element.
    // Closed new shapes still require full identity matching.
    if matches!(
        observed,
        LivePaperPrimitive::Circle { .. } | LivePaperPrimitive::Polyline { closed: true, .. }
    ) {
        return false;
    }
    let points: Vec<[f64; 2]> = match observed {
        LivePaperPrimitive::Point { position_mm, .. } => vec![*position_mm],
        LivePaperPrimitive::Line {
            start_mm, end_mm, ..
        } => (0..=8)
            .map(|i| {
                let t = i as f64 / 8.0;
                [
                    start_mm[0] + t * (end_mm[0] - start_mm[0]),
                    start_mm[1] + t * (end_mm[1] - start_mm[1]),
                ]
            })
            .collect(),
        LivePaperPrimitive::Polyline { points_mm, .. } => points_mm
            .iter()
            .step_by((points_mm.len() / 128).max(1))
            .copied()
            .chain(points_mm.last().copied())
            .collect(),
        _ => return false,
    };
    points.iter().all(|p| match accepted {
        LivePaperPrimitive::Point { position_mm, .. } => {
            distance(*p, *position_mm) <= MATCH_TOLERANCE_MM
        }
        LivePaperPrimitive::Line {
            start_mm, end_mm, ..
        } => path_distance(&[*p], &[*start_mm, *end_mm], false) <= MATCH_TOLERANCE_MM,
        LivePaperPrimitive::Circle {
            center_mm,
            radius_mm,
            ..
        } => (distance(*p, *center_mm) - radius_mm).abs() <= MATCH_TOLERANCE_MM,
        LivePaperPrimitive::Polyline {
            points_mm, closed, ..
        } => path_distance(&[*p], points_mm, *closed) <= MATCH_TOLERANCE_MM,
    })
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn path_distance(points: &[[f64; 2]], path: &[[f64; 2]], closed: bool) -> f64 {
    points
        .iter()
        .map(|p| {
            (0..path.len().saturating_sub(usize::from(!closed)))
                .map(|i| {
                    let a = path[i];
                    let b = path[(i + 1) % path.len()];
                    let dx = b[0] - a[0];
                    let dy = b[1] - a[1];
                    let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy)
                        / (dx * dx + dy * dy).max(1e-12))
                    .clamp(0.0, 1.0);
                    distance(*p, [a[0] + t * dx, a[1] + t * dy])
                })
                .fold(f64::INFINITY, f64::min)
        })
        .fold(0.0, f64::max)
}

#[derive(Clone)]
struct Track {
    original: LivePaperPrimitive,
    latest: LivePaperPrimitive,
    hits: u32,
    last_sequence: u64,
    needs_confirmation: bool,
    manually_edited: bool,
    missing_observations: u16,
}

#[derive(Clone)]
struct ViewDraft {
    accepted: Vec<LivePaperPrimitive>,
    tracks: BTreeMap<String, Track>,
    aliases: BTreeMap<String, Vec<LivePaperPrimitive>>,
    rejected: Vec<LivePaperPrimitive>,
}

#[derive(Clone)]
struct Session {
    token: String,
    snapshot: LivePaperSnapshot,
    tracks: BTreeMap<String, Track>,
    // Original observations also identify an element after a deliberate edit.
    aliases: BTreeMap<String, Vec<LivePaperPrimitive>>,
    rejected: Vec<LivePaperPrimitive>,
    other_views: BTreeMap<String, ViewDraft>,
    latest_received: u64,
    media_epoch: u64,
    media_available: bool,
}

impl Session {
    fn new(base_url: &str) -> Self {
        let session_id = uuid::Uuid::new_v4().to_string();
        let token = uuid::Uuid::new_v4().to_string();
        let base_url = base_url.trim_end_matches('/');
        let phone_url = format!("{base_url}/live-paper?session={session_id}&token={token}");
        let phone_qr_data_url = qrcode::QrCode::new(phone_url.as_bytes())
            .map(|code| {
                use base64::Engine;
                let svg = code
                    .render::<qrcode::render::svg::Color>()
                    .min_dimensions(200, 200)
                    .build();
                format!(
                    "data:image/svg+xml;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(svg)
                )
            })
            .unwrap_or_default();
        Self {
            snapshot: LivePaperSnapshot {
                phone_url,
                phone_qr_data_url,
                template_url: format!(
                    "{base_url}/api/live-paper/{session_id}/template.pdf?token={token}"
                ),
                template_data_url: {
                    use base64::Engine;
                    format!(
                        "data:application/pdf;base64,{}",
                        base64::engine::general_purpose::STANDARD
                            .encode(include_bytes!("../../assets/live-paper-template.pdf"))
                    )
                },
                session_id,
                scope_key: String::new(),
                view: crate::contracts::SketchView::Front,
                trust_url: None,
                state: "waiting".into(),
                detail: Some(
                    "Open the phone link, allow camera access, and show all four printed markers."
                        .into(),
                ),
                sequence: 0,
                width_mm: 160.0,
                height_mm: 220.0,
                preview_data_url: None,
                camera_preview_data_url: None,
                preview_kind: "camera".into(),
                accepted: Vec::new(),
                pending: Vec::new(),
                diagnostics: Vec::new(),
            },
            token,
            tracks: BTreeMap::new(),
            aliases: BTreeMap::new(),
            rejected: Vec::new(),
            other_views: BTreeMap::new(),
            latest_received: 0,
            media_epoch: 0,
            media_available: true,
        }
    }

    fn refresh_template(&mut self) -> Result<(), String> {
        use base64::Engine;
        self.snapshot.template_data_url = format!(
            "data:application/pdf;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(
                super::live_paper_vision::template_pdf_for_view(&self.snapshot.view)?
            )
        );
        Ok(())
    }

    fn set_view(&mut self, view: crate::contracts::SketchView) -> Result<(), String> {
        if view == crate::contracts::SketchView::Custom {
            return Err("Choose FRONT, TOP, or SIDE for the camera.".into());
        }
        if self.snapshot.view == view {
            return self.refresh_template();
        }
        let old = ViewDraft {
            accepted: std::mem::take(&mut self.snapshot.accepted),
            tracks: std::mem::take(&mut self.tracks),
            aliases: std::mem::take(&mut self.aliases),
            rejected: std::mem::take(&mut self.rejected),
        };
        self.other_views
            .insert(view_key(&self.snapshot.view).into(), old);
        if let Some(draft) = self.other_views.remove(view_key(&view)) {
            self.snapshot.accepted = draft.accepted;
            self.tracks = draft.tracks;
            self.aliases = draft.aliases;
            self.rejected = draft.rejected;
        }
        self.snapshot.view = view;
        self.snapshot.preview_data_url = None;
        self.snapshot.preview_kind = if self.snapshot.state == "stopped" {
            "paper"
        } else {
            "camera"
        }
        .into();
        self.snapshot.diagnostics.clear();
        if self.snapshot.state != "stopped" && self.media_available {
            self.snapshot.state = if self.snapshot.camera_preview_data_url.is_some() {
                "streaming"
            } else {
                "waiting"
            }
            .into();
            self.snapshot.detail = Some(format!(
                "Camera assigned to {}. Show that view's four printed markers to calibrate.",
                view_key(&self.snapshot.view).to_uppercase()
            ));
        }
        self.media_epoch = self.media_epoch.wrapping_add(1);
        self.refresh_pending();
        self.refresh_template()
    }

    fn refresh_pending(&mut self) {
        self.snapshot.pending = self.tracks.values().map(|t| t.latest.clone()).collect();
    }

    fn observe(&mut self, sequence: u64, candidates: Vec<(LivePaperPrimitive, bool)>) {
        if sequence <= self.snapshot.sequence || self.snapshot.state == "stopped" {
            return;
        }
        let previous = self.snapshot.sequence;
        let mut seen = BTreeSet::new();
        for (mut primitive, uncertain) in candidates.into_iter().take(MAX_ELEMENTS) {
            if !primitive.valid()
                || self
                    .rejected
                    .iter()
                    .any(|p| p.matches(&primitive) || covers(p, &primitive))
            {
                continue;
            }
            let accepted_match = self
                .snapshot
                .accepted
                .iter()
                .any(|p| p.matches(&primitive) || covers(p, &primitive))
                || self
                    .aliases
                    .values()
                    .flatten()
                    .any(|p| p.matches(&primitive) || covers(p, &primitive));
            if accepted_match {
                continue;
            }
            let existing = self
                .tracks
                .iter()
                .find(|(_, t)| t.original.matches(&primitive) || t.latest.matches(&primitive))
                .map(|(id, _)| id.clone());
            if let Some(id) = existing {
                if !seen.insert(id.clone()) {
                    continue;
                }
                let Some(track) = self.tracks.get_mut(&id) else {
                    continue;
                };
                primitive.set_id(id.clone());
                track.hits = if track.last_sequence == previous {
                    track.hits.saturating_add(1)
                } else {
                    1
                };
                track.last_sequence = sequence;
                track.missing_observations = 0;
                track.needs_confirmation |= uncertain;
                if !track.manually_edited {
                    track.latest = primitive;
                }
                if track.hits >= STABLE_OBSERVATIONS
                    && !track.needs_confirmation
                    && self.snapshot.accepted.len() < MAX_ELEMENTS
                {
                    let Some(track) = self.tracks.remove(&id) else {
                        continue;
                    };
                    self.aliases.insert(id, vec![track.original]);
                    self.snapshot.accepted.push(track.latest);
                }
            } else if self.tracks.len() < MAX_ELEMENTS {
                let id = uuid::Uuid::new_v4().to_string();
                primitive.set_id(id.clone());
                seen.insert(id.clone());
                self.tracks.insert(
                    id,
                    Track {
                        original: primitive.clone(),
                        latest: primitive,
                        hits: 1,
                        last_sequence: sequence,
                        needs_confirmation: uncertain,
                        manually_edited: false,
                        missing_observations: 0,
                    },
                );
            }
        }
        self.tracks.retain(|_, track| {
            if track.last_sequence != sequence {
                track.missing_observations = track.missing_observations.saturating_add(1);
            }
            track.manually_edited || track.missing_observations < 45
        });
        self.snapshot.sequence = sequence;
        self.refresh_pending();
    }

    fn confirm(&mut self, id: &str) -> Result<(), String> {
        if self.snapshot.accepted.len() >= MAX_ELEMENTS {
            return Err("Sketch element limit reached; remove geometry before confirming.".into());
        }
        let track = self
            .tracks
            .remove(id)
            .ok_or("Candidate no longer exists; refresh the paper view.")?;
        self.aliases.insert(id.into(), vec![track.original]);
        self.snapshot.accepted.push(track.latest);
        self.refresh_pending();
        Ok(())
    }

    fn edit(&mut self, primitive: LivePaperPrimitive) -> Result<(), String> {
        if !primitive.valid() {
            return Err("Geometry needs finite coordinates, distinct line endpoints, and a positive circle radius.".into());
        }
        let id = primitive.id().to_owned();
        if let Some(existing) = self.snapshot.accepted.iter_mut().find(|p| p.id() == id) {
            // Keep both the source and the edited position authoritative for frame matching.
            let aliases = self.aliases.entry(id).or_default();
            if aliases.len() < 32 {
                aliases.push(existing.clone());
            }
            *existing = primitive;
        } else if let Some(track) = self.tracks.get_mut(&id) {
            track.latest = primitive;
            track.needs_confirmation = true;
            track.manually_edited = true;
        } else {
            return Err("Element no longer exists; refresh the paper view.".into());
        }
        self.refresh_pending();
        Ok(())
    }

    fn reject(&mut self, id: &str) -> Result<(), String> {
        if let Some(track) = self.tracks.remove(id) {
            self.rejected.extend([track.original, track.latest]);
        } else if let Some(index) = self.snapshot.accepted.iter().position(|p| p.id() == id) {
            self.rejected.push(self.snapshot.accepted.remove(index));
            self.rejected
                .extend(self.aliases.remove(id).unwrap_or_default());
        } else {
            return Err("Element no longer exists; refresh the paper view.".into());
        }
        // Bounded session memory; explicit new capture clears rejection history.
        if self.rejected.len() > 4096 {
            self.rejected.drain(..self.rejected.len() - 4096);
        }
        self.refresh_pending();
        Ok(())
    }
}

struct Frame {
    session_id: String,
    sequence: u64,
    media_epoch: u64,
    image: image::DynamicImage,
}
#[derive(Default)]
struct HubState {
    session: Option<Session>,
    queued: Option<Frame>,
    processing: bool,
    storage_path: Option<std::path::PathBuf>,
    saved_draft: Option<super::live_paper_store::PaperDraft>,
}

pub struct LivePaperHub {
    inner: Mutex<HubState>,
    events: broadcast::Sender<LivePaperSnapshot>,
}

impl Default for LivePaperHub {
    fn default() -> Self {
        Self {
            inner: Mutex::new(HubState::default()),
            events: broadcast::channel(8).0,
        }
    }
}

static HUB: LazyLock<Arc<LivePaperHub>> = LazyLock::new(|| Arc::new(LivePaperHub::default()));
pub fn hub() -> Arc<LivePaperHub> {
    Arc::clone(&HUB)
}

impl LivePaperHub {
    pub fn configure_storage(&self, path: std::path::PathBuf) -> Result<(), String> {
        // A restore error preserves the file; an explicit new capture can replace its draft.
        self.inner
            .lock()
            .map_err(|_| "Paper capture state unavailable.")?
            .storage_path = Some(path.clone());
        let saved = super::live_paper_store::load(&path)?;
        if let Some(draft) = &saved {
            if draft.view == crate::contracts::SketchView::Custom
                || draft.scope_key.len() > 4096
                || draft.accepted.len() > MAX_ELEMENTS
                || draft.accepted.iter().any(|p| !p.valid())
                || draft.other_views.len() > 2
                || draft.other_views.iter().any(|v| {
                    v.view == draft.view
                        || v.view == crate::contracts::SketchView::Custom
                        || v.accepted.len() > MAX_ELEMENTS
                        || v.accepted.iter().any(|p| !p.valid())
                })
                || draft
                    .other_views
                    .windows(2)
                    .any(|views| views[0].view == views[1].view)
            {
                return Err(
                    "Accepted paper draft contains invalid geometry; its file was preserved."
                        .into(),
                );
            }
        }
        let mut state = self
            .inner
            .lock()
            .map_err(|_| "Paper capture state unavailable.")?;
        state.storage_path = Some(path);
        state.saved_draft = saved.clone();
        if state.session.is_none() {
            if let Some(draft) = saved.filter(|draft| {
                !draft.accepted.is_empty()
                    || draft.other_views.iter().any(|v| !v.accepted.is_empty())
            }) {
                let mut session = Session::new("");
                session.snapshot.scope_key = draft.scope_key;
                session.snapshot.view = draft.view;
                session.snapshot.accepted = draft.accepted;
                for other in draft.other_views {
                    session.other_views.insert(
                        view_key(&other.view).into(),
                        ViewDraft {
                            accepted: other.accepted,
                            tracks: BTreeMap::new(),
                            aliases: BTreeMap::new(),
                            rejected: Vec::new(),
                        },
                    );
                }
                session.snapshot.preview_kind = "paper".into();
                session.refresh_template()?;
                session.snapshot.state = "stopped".into();
                session.snapshot.detail = Some("Restored accepted paper draft. Geometry remains editable; start a new capture to reconnect a phone.".into());
                state.session = Some(session);
            }
        }
        Ok(())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LivePaperSnapshot> {
        self.events.subscribe()
    }
    fn emit(&self, snapshot: &LivePaperSnapshot) {
        let _ = self.events.send(snapshot.clone());
    }
    pub fn current(&self) -> Option<LivePaperSnapshot> {
        self.inner
            .lock()
            .ok()?
            .session
            .as_ref()
            .map(|s| s.snapshot.clone())
    }
    pub fn start(&self, base_url: &str) -> LivePaperSnapshot {
        self.start_scoped(base_url, String::new(), crate::contracts::SketchView::Front)
    }
    pub fn start_scoped(
        &self,
        base_url: &str,
        scope_key: String,
        view: crate::contracts::SketchView,
    ) -> LivePaperSnapshot {
        let mut state = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let mut session = Session::new(base_url);
        session.snapshot.scope_key = scope_key.clone();
        if let Some(previous) = state
            .session
            .take()
            .filter(|previous| previous.snapshot.scope_key == scope_key)
        {
            session.snapshot.view = previous.snapshot.view;
            session.snapshot.accepted = previous.snapshot.accepted;
            session.tracks = previous.tracks;
            session.aliases = previous.aliases;
            session.rejected = previous.rejected;
            session.other_views = previous.other_views;
        }
        if let Err(error) = session.set_view(view) {
            session.snapshot.state = "error".into();
            session.snapshot.detail = Some(error);
        }
        // New session identity already invalidates every previous camera frame.
        session.media_epoch = 0;
        if let Err(error) = session.refresh_template() {
            session.snapshot.state = "error".into();
            session.snapshot.detail = Some(error);
        }
        let snapshot = session.snapshot.clone();
        state.session = Some(session);
        state.queued = None;
        if let Err(error) = persist_locked(&mut state) {
            if let Some(session) = state.session.as_mut() {
                session.snapshot.state = "error".into();
                session.snapshot.detail = Some(error);
            }
        }
        let snapshot = state
            .session
            .as_ref()
            .map(|s| s.snapshot.clone())
            .unwrap_or(snapshot);
        self.emit(&snapshot);
        drop(state);
        snapshot
    }
    fn session_matches(&self, session_id: &str) -> Result<(), String> {
        let state = self
            .inner
            .lock()
            .map_err(|_| "Paper capture state unavailable.")?;
        active_session(&state, session_id).map(|_| ())
    }
    pub fn authorize(&self, session_id: &str, token: &str) -> bool {
        self.inner
            .lock()
            .ok()
            .and_then(|state| {
                active_session(&state, session_id)
                    .ok()
                    .map(|s| s.token == token)
            })
            .unwrap_or(false)
    }
    pub fn set_view(
        &self,
        session_id: &str,
        view: crate::contracts::SketchView,
    ) -> Result<LivePaperSnapshot, String> {
        self.mutate(session_id, |session| session.set_view(view))
    }
    pub fn set_trust_url(
        &self,
        session_id: &str,
        trust_url: Option<String>,
    ) -> Result<LivePaperSnapshot, String> {
        self.mutate(session_id, |s| {
            s.snapshot.trust_url = trust_url;
            Ok(())
        })
    }
    pub fn stop(&self, session_id: &str) -> Result<LivePaperSnapshot, String> {
        let mut state = self
            .inner
            .lock()
            .map_err(|_| "Paper capture state unavailable.")?;
        let session = state
            .session
            .as_mut()
            .filter(|s| s.snapshot.session_id == session_id)
            .ok_or("Paper capture was replaced.")?;
        session.snapshot.state = "stopped".into();
        session.snapshot.detail =
            Some("Camera stopped. Accepted geometry remains editable.".into());
        let snapshot = session.snapshot.clone();
        state.queued = None;
        self.emit(&snapshot);
        drop(state);
        Ok(snapshot)
    }
    fn mutate(
        &self,
        session_id: &str,
        action: impl FnOnce(&mut Session) -> Result<(), String>,
    ) -> Result<LivePaperSnapshot, String> {
        let mut state = self
            .inner
            .lock()
            .map_err(|_| "Paper capture state unavailable.")?;
        let session = state
            .session
            .as_mut()
            .filter(|s| s.snapshot.session_id == session_id)
            .ok_or("Paper capture was replaced.")?;
        let previous = session.clone();
        action(session)?;
        if let Err(error) = persist_locked(&mut state) {
            state.session = Some(previous);
            return Err(error);
        }
        let snapshot = state
            .session
            .as_ref()
            .ok_or("Paper capture unavailable.")?
            .snapshot
            .clone();
        self.emit(&snapshot);
        drop(state);
        Ok(snapshot)
    }
    pub fn confirm(&self, session_id: &str, id: &str) -> Result<LivePaperSnapshot, String> {
        self.mutate(session_id, |s| s.confirm(id))
    }
    pub fn edit(
        &self,
        session_id: &str,
        primitive: LivePaperPrimitive,
    ) -> Result<LivePaperSnapshot, String> {
        self.mutate(session_id, |s| s.edit(primitive))
    }
    pub fn reject(&self, session_id: &str, id: &str) -> Result<LivePaperSnapshot, String> {
        self.mutate(session_id, |s| s.reject(id))
    }
    pub async fn set_transport_state(
        &self,
        session_id: &str,
        status: &str,
        detail: Option<String>,
    ) {
        if self.session_matches(session_id).is_err() {
            return;
        }
        let _ = self.mutate(session_id, |s| {
            if s.snapshot.state == "stopped" { return Ok(()); }
            let was_available = s.media_available;
            s.media_available = !matches!(status, "error" | "disconnected");
            if !s.media_available { s.media_epoch = s.media_epoch.wrapping_add(1); }
            // Transport connectivity alone does not establish a visible calibrated sheet.
            if status == "streaming" {
                if s.snapshot.sequence > 0 && was_available { return Ok(()); }
                s.snapshot.state = "connecting".into();
                s.snapshot.detail = Some("Video connected. Show all four paper markers to calibrate the sketch.".into());
            } else if status == "disconnected" {
                s.snapshot.state = "error".into();
                s.snapshot.detail = Some("Camera disconnected. Accepted geometry is preserved; reconnect the phone or restart capture.".into());
            } else {
                s.snapshot.state = status.into();
                s.snapshot.detail = detail;
            }
            Ok(())
        });
    }
    pub async fn ingest_frame(
        self: &Arc<Self>,
        session_id: &str,
        sequence: u64,
        image: image::DynamicImage,
    ) -> Result<(), String> {
        if image.width() == 0
            || image.height() == 0
            || u64::from(image.width()) * u64::from(image.height()) > 4_000_000
        {
            return Err("Camera frame exceeds the 4 megapixel live processing limit; choose a lower camera resolution.".into());
        }
        let mut state = self
            .inner
            .lock()
            .map_err(|_| "Paper capture state unavailable.")?;
        active_session(&state, session_id)?;
        let Some(session) = state.session.as_mut() else {
            return Err("Start paper capture first.".into());
        };
        if sequence <= session.latest_received {
            return Ok(());
        }
        if !session.media_available {
            return Err(
                "Camera paused or disconnected; reconnect to resume geometry updates.".into(),
            );
        }
        session.latest_received = sequence;
        let media_epoch = session.media_epoch;
        state.queued = Some(Frame {
            session_id: session_id.into(),
            sequence,
            media_epoch,
            image,
        });
        if !state.processing {
            state.processing = true;
            let service = Arc::clone(self);
            tokio::spawn(async move {
                service.process().await;
            });
        }
        Ok(())
    }
    async fn process(self: Arc<Self>) {
        loop {
            let frame = {
                let mut state = self.inner.lock().unwrap_or_else(|p| p.into_inner());
                match state.queued.take() {
                    Some(frame) => frame,
                    None => {
                        state.processing = false;
                        return;
                    }
                }
            };
            let id = frame.session_id.clone();
            let sequence = frame.sequence;
            let media_epoch = frame.media_epoch;
            let service = Arc::clone(&self);
            let failure_id = id.clone();
            let result = tokio::task::spawn_blocking(move || {
                if let Ok(preview) = camera_preview(&frame.image) {
                    service.publish_camera(&id, sequence, media_epoch, preview);
                }
                let result = super::live_paper_vision::recognize_live_paper(&frame.image);
                service.finish_frame(&id, sequence, media_epoch, result);
            })
            .await;
            if let Err(error) = result {
                self.set_transport_state(
                    &failure_id,
                    "error",
                    Some(format!("Paper recognition worker failed: {error}")),
                )
                .await;
            }
        }
    }

    fn publish_camera(&self, id: &str, sequence: u64, media_epoch: u64, preview: String) {
        let mut state = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let Some(session) = state.session.as_mut().filter(|s| {
            s.snapshot.session_id == id
                && s.snapshot.state != "stopped"
                && s.media_available
                && s.media_epoch == media_epoch
                && sequence > s.snapshot.sequence
        }) else {
            return;
        };
        session.snapshot.camera_preview_data_url = Some(preview);
        if session.snapshot.preview_kind == "camera" {
            if matches!(session.snapshot.state.as_str(), "waiting" | "connecting") {
                session.snapshot.state = "streaming".into();
                session.snapshot.detail = Some(format!(
                    "Live camera. Align all four {} markers to recognize a metric sketch.",
                    view_key(&session.snapshot.view).to_uppercase()
                ));
            }
            self.emit(&session.snapshot);
        }
    }

    fn finish_frame(
        &self,
        id: &str,
        sequence: u64,
        media_epoch: u64,
        result: Result<super::live_paper_vision::LivePaperObservation, String>,
    ) {
        let mut state = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let Some(session) = state.session.as_mut().filter(|s| {
            s.snapshot.session_id == id
                && s.snapshot.state != "stopped"
                && s.media_available
                && s.media_epoch == media_epoch
                && sequence > s.snapshot.sequence
        }) else {
            return;
        };
        match result {
            Ok(observation) if observation.view != session.snapshot.view => {
                session.snapshot.state = "templateMismatch".into();
                session.snapshot.preview_kind = "camera".into();
                session.snapshot.detail=Some(format!("{} template detected; camera is assigned to {}. Select the matching view or use its printed template.",view_key(&observation.view).to_uppercase(),view_key(&session.snapshot.view).to_uppercase()));
                session.snapshot.sequence = sequence;
                for track in session.tracks.values_mut() {
                    track.hits = 0;
                }
            }
            Ok(observation) => {
                session.snapshot.preview_kind = "paper".into();
                session.snapshot.state = "streaming".into();
                session.snapshot.detail = None;
                session.snapshot.preview_data_url = Some(observation.preview_data_url);
                session.snapshot.width_mm = observation.width_mm;
                session.snapshot.height_mm = observation.height_mm;
                session.snapshot.diagnostics = observation.diagnostics;
                session.observe(
                    sequence,
                    observation
                        .primitives
                        .into_iter()
                        .map(|p| (p.primitive, p.needs_confirmation))
                        .collect(),
                );
            }
            Err(error) => {
                session.snapshot.preview_kind = "camera".into();
                session.snapshot.state = "markerLost".into();
                session.snapshot.detail = Some(error);
                session.snapshot.sequence = sequence;
                for track in session.tracks.values_mut() {
                    track.hits = 0;
                }
            }
        }
        if let Err(error) = persist_locked(&mut state) {
            if let Some(session) = state.session.as_mut() {
                session.snapshot.state = "error".into();
                session.snapshot.detail = Some(error);
                session.media_available = false;
            }
        }
        let Some(session) = state.session.as_ref() else {
            return;
        };
        let snapshot = session.snapshot.clone();
        self.emit(&snapshot);
        drop(state);
    }
}

fn persist_locked(state: &mut HubState) -> Result<(), String> {
    let Some(path) = &state.storage_path else {
        return Ok(());
    };
    let Some(session) = &state.session else {
        return Ok(());
    };
    let draft = super::live_paper_store::PaperDraft {
        scope_key: session.snapshot.scope_key.clone(),
        view: session.snapshot.view.clone(),
        accepted: session.snapshot.accepted.clone(),
        other_views: session
            .other_views
            .iter()
            .map(|(view, draft)| super::live_paper_store::PaperViewDraft {
                view: parse_view(view),
                accepted: draft.accepted.clone(),
            })
            .collect(),
    };
    if state.saved_draft.as_ref() == Some(&draft) {
        return Ok(());
    }
    super::live_paper_store::save(path, &draft)?;
    state.saved_draft = Some(draft);
    Ok(())
}

fn view_key(view: &crate::contracts::SketchView) -> &'static str {
    match view {
        crate::contracts::SketchView::Front => "front",
        crate::contracts::SketchView::Top => "top",
        crate::contracts::SketchView::Side => "side",
        crate::contracts::SketchView::Custom => "custom",
    }
}
fn parse_view(view: &str) -> crate::contracts::SketchView {
    match view {
        "top" => crate::contracts::SketchView::Top,
        "side" => crate::contracts::SketchView::Side,
        _ => crate::contracts::SketchView::Front,
    }
}
fn camera_preview(image: &image::DynamicImage) -> Result<String, String> {
    use base64::Engine;
    let mut buffer = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image.thumbnail(960, 960).to_rgb8())
        .write_to(&mut buffer, image::ImageFormat::Jpeg)
        .map_err(|e| format!("Could not display live camera: {e}"))?;
    Ok(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(buffer.into_inner())
    ))
}

fn active_session<'a>(state: &'a HubState, id: &str) -> Result<&'a Session, String> {
    state
        .session
        .as_ref()
        .filter(|s| s.snapshot.session_id == id && s.snapshot.state != "stopped")
        .ok_or_else(|| {
            "Paper capture stopped or was replaced; start a new camera connection.".into()
        })
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LivePaperProfile {
    pub view: crate::contracts::SketchView,
    pub loops: Vec<super::profile_authoring::ProfileLoop>,
    pub sketch_loops: Vec<super::profile_authoring::ProfileSketchLoop>,
    pub diagnostics: Vec<String>,
}

pub fn prepare_profile(snapshot: &LivePaperSnapshot) -> Result<LivePaperProfile, String> {
    use super::drawing_import::{DrawingPrimitive, DrawingProfileRole};
    use super::profile_authoring::{
        ProfileLoop, ProfileLoopRole, ProfileSketchGeometry, ProfileSketchLoop,
    };
    let primitives = snapshot
        .accepted
        .iter()
        .filter_map(|p| match p {
            LivePaperPrimitive::Point { .. } => None,
            LivePaperPrimitive::Line {
                id,
                start_mm,
                end_mm,
                residual_mm,
            } => Some(DrawingPrimitive::Line {
                id: id.clone(),
                start_mm: *start_mm,
                end_mm: *end_mm,
                residual_mm: *residual_mm,
            }),
            LivePaperPrimitive::Circle {
                id,
                center_mm,
                radius_mm,
                residual_mm,
            } => Some(DrawingPrimitive::Circle {
                id: id.clone(),
                center_mm: *center_mm,
                radius_mm: *radius_mm,
                residual_mm: *residual_mm,
            }),
            LivePaperPrimitive::Polyline {
                id,
                points_mm,
                closed,
                residual_mm,
            } => Some(DrawingPrimitive::Polyline {
                id: id.clone(),
                points_mm: points_mm.clone(),
                closed: *closed,
                residual_mm: *residual_mm,
            }),
        })
        .collect::<Vec<_>>();
    let reviewed = super::drawing_import::review_profile_primitives(&primitives);
    if reviewed.closed_primitives.is_empty() {
        return Err("Accept or repair a closed outline or circle before using a profile.".into());
    }
    // Reclassify the normalized closed primitives; review may join several source lines.
    let profiles = super::drawing_import::assemble_profile_candidates(&reviewed.closed_primitives);
    if let Some(issue) = profiles.iter().flat_map(|p| p.issues.iter()).next() {
        return Err(issue.clone());
    }
    let mut loops = Vec::new();
    let mut sketch_loops = Vec::new();
    for (primitive, profile) in reviewed.closed_primitives.iter().zip(&profiles) {
        let role = match profile.role {
            DrawingProfileRole::Outer => ProfileLoopRole::Outer,
            DrawingProfileRole::Hole => ProfileLoopRole::Hole,
        };
        let parent_index = profile
            .parent_id
            .as_ref()
            .and_then(|id| profiles.iter().position(|p| &p.id == id));
        let (points, geometry) = match primitive {
            DrawingPrimitive::Circle {
                center_mm,
                radius_mm,
                ..
            } => {
                let points = (0..64)
                    .map(|i| {
                        let angle = i as f64 * std::f64::consts::TAU / 64.0;
                        [
                            center_mm[0] + radius_mm * angle.cos(),
                            center_mm[1] + radius_mm * angle.sin(),
                        ]
                    })
                    .collect();
                (
                    points,
                    ProfileSketchGeometry::Circle {
                        center_mm: *center_mm,
                        radius_mm: *radius_mm,
                    },
                )
            }
            DrawingPrimitive::Polyline { points_mm, .. } => (
                points_mm.clone(),
                ProfileSketchGeometry::Polygon {
                    points_mm: points_mm.clone(),
                },
            ),
            DrawingPrimitive::Line { .. } => continue,
        };
        loops.push(ProfileLoop {
            points,
            role,
            parent_index,
        });
        sketch_loops.push(ProfileSketchLoop {
            geometry,
            role,
            parent_index,
        });
    }
    Ok(LivePaperProfile {
        view: snapshot.view.clone(),
        loops,
        sketch_loops,
        diagnostics: reviewed.diagnostics,
    })
}
