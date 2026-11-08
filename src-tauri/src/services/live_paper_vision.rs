//! Fiducial-calibrated, transient paper observations. No model history writes.
use super::live_paper::LivePaperPrimitive;
use image::DynamicImage;

#[derive(Debug)]
pub struct LivePaperCandidate {
    pub primitive: LivePaperPrimitive,
    pub needs_confirmation: bool,
}
#[derive(Debug)]
pub struct LivePaperObservation {
    pub preview_data_url: String,
    pub width_mm: f64,
    pub height_mm: f64,
    pub primitives: Vec<LivePaperCandidate>,
    pub diagnostics: Vec<String>,
}
const WIDTH_MM: f64 = 160.0;
const HEIGHT_MM: f64 = 220.0;
const PIXELS_PER_MM: f64 = 3.0;
const WIDTH_PX: u32 = 480;
const HEIGHT_PX: u32 = 660;
const MARKER_CENTERS_MM: [(f32, f32); 4] =
    [(17.0, 30.0), (193.0, 30.0), (193.0, 266.0), (17.0, 266.0)];

#[path = "vendor/skeleton_tracing.rs"]
mod skeleton_tracing;

use super::drawing_import::{fit_primitives, DrawingPrimitive};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use image::{GrayImage, ImageFormat, Luma};
use imageproc::{
    contrast::{otsu_level, threshold, ThresholdType},
    geometric_transformations::{warp_into, Border, Interpolation, Projection},
    region_labelling::{connected_components, Connectivity},
};
use locus_core::{Detector, ImageView, TagFamily};
use std::io::Cursor;

/// Detect the four printed identifiers afresh. A lost marker never reuses stale scale.
/// Pixel work is bounded; caller runs this synchronous operation on a blocking worker.
pub fn recognize_live_paper(image: &DynamicImage) -> Result<LivePaperObservation, String> {
    if image.width() < 64
        || image.height() < 64
        || u64::from(image.width()) * u64::from(image.height()) > 4_000_000
    {
        return Err(
            "Camera frame must contain 64 pixels per side and at most 4 million pixels.".into(),
        );
    }
    let resized = image.thumbnail(1280, 1280).to_luma8();
    let projection = paper_projection(&resized)?;
    let mut rectified = GrayImage::from_pixel(WIDTH_PX, HEIGHT_PX, Luma([255]));
    warp_into(
        &resized,
        projection,
        Interpolation::Bilinear,
        Border::Constant(Luma([255])),
        &mut rectified,
    );
    let level = otsu_level(&rectified).clamp(40, 185);
    let binary = threshold(&rectified, level, ThresholdType::BinaryInverted);
    let labels = connected_components(&binary, Connectivity::Eight, Luma([0u8]));
    let max_label = labels.pixels().map(|p| p[0]).max().unwrap_or(0) as usize;
    if max_label > 4096 {
        return Err("Too much image noise; improve lighting and focus on the marker sheet.".into());
    }
    let mut bounds = vec![(u32::MAX, u32::MAX, 0u32, 0u32, 0u32); max_label + 1];
    for (x, y, label) in labels.enumerate_pixels() {
        if label[0] == 0 {
            continue;
        }
        let b = &mut bounds[label[0] as usize];
        b.0 = b.0.min(x);
        b.1 = b.1.min(y);
        b.2 = b.2.max(x);
        b.3 = b.3.max(y);
        b.4 += 1;
    }
    let mut primitives = Vec::new();
    let mut diagnostics = Vec::new();
    for (label, &(left, top, right, bottom, area)) in bounds.iter().enumerate().skip(1) {
        if area < 5 {
            continue;
        }
        if primitives.len() >= 128 {
            diagnostics.push("Recognition reached 128 elements; use a simpler drawing.".into());
            break;
        }
        let (width, height) = (right - left + 1, bottom - top + 1);
        if area > WIDTH_PX * HEIGHT_PX / 12
            || area > 200
                && width > 12
                && height > 12
                && area as f64 / f64::from(width * height) > 0.65
        {
            diagnostics.push("A dense ink region or occlusion is held for review; accepted geometry is preserved.".into());
            continue;
        }
        if width <= 8 && height <= 8 && area >= 7 {
            primitives.push(LivePaperCandidate {
                primitive: LivePaperPrimitive::Point {
                    id: format!("point-{label}"),
                    position_mm: [
                        f64::from(left + right) / 2.0 / PIXELS_PER_MM,
                        HEIGHT_MM - f64::from(top + bottom) / 2.0 / PIXELS_PER_MM,
                    ],
                    residual_mm: f64::from(width.max(height)) / 2.0 / PIXELS_PER_MM,
                },
                needs_confirmation: false,
            });
            continue;
        }
        if width < 6 && height < 6 {
            continue;
        }
        let (crop_width, crop_height) = (width + 6, height + 6);
        let mut skeleton = vec![0u8; (crop_width * crop_height) as usize];
        for y in top..=bottom {
            for x in left..=right {
                if labels.get_pixel(x, y)[0] as usize == label {
                    skeleton[((y - top + 3) * crop_width + x - left + 3) as usize] = 1;
                }
            }
        }
        skeleton_tracing::thinning_zs(&mut skeleton, crop_width as usize, crop_height as usize);
        let bool_skeleton = skeleton.iter().map(|v| *v != 0).collect::<Vec<_>>();
        let (fitted, issues) = fit_primitives(
            &bool_skeleton,
            crop_width,
            crop_height,
            1.0 / PIXELS_PER_MM,
            0.65,
        );
        let dx = (f64::from(left) - 3.0) / PIXELS_PER_MM;
        let dy = HEIGHT_MM - (f64::from(top) - 3.0 + f64::from(crop_height)) / PIXELS_PER_MM;
        if fitted.is_empty() && !issues.is_empty() {
            diagnostics.push("A junction or crossing needs confirmation; branches remain separate editable strokes.".into());
            let paths = skeleton_tracing::trace_skeleton(
                &mut skeleton,
                crop_width as usize,
                crop_height as usize,
                0,
                0,
                crop_width as usize,
                crop_height as usize,
                10,
                64,
            );
            for (index, path) in paths.into_iter().enumerate().take(32) {
                if path.len() < 2 {
                    continue;
                }
                let points = path
                    .into_iter()
                    .map(|p| {
                        [
                            dx + p[0] as f64 / PIXELS_PER_MM,
                            dy + (f64::from(crop_height) - p[1] as f64) / PIXELS_PER_MM,
                        ]
                    })
                    .collect::<Vec<_>>();
                if path_length(&points) < 2.0 {
                    continue;
                }
                primitives.push(LivePaperCandidate {
                    primitive: LivePaperPrimitive::Polyline {
                        id: format!("junction-{label}-{index}"),
                        points_mm: points,
                        closed: false,
                        residual_mm: 0.65,
                    },
                    needs_confirmation: true,
                });
            }
        } else {
            diagnostics.extend(issues);
            for primitive in fitted {
                let translate = |p: [f64; 2]| [p[0] + dx, p[1] + dy];
                let primitive = match primitive {
                    DrawingPrimitive::Line {
                        id,
                        start_mm,
                        end_mm,
                        residual_mm,
                    } => LivePaperPrimitive::Line {
                        id: format!("{label}-{id}"),
                        start_mm: translate(start_mm),
                        end_mm: translate(end_mm),
                        residual_mm,
                    },
                    DrawingPrimitive::Circle {
                        id,
                        center_mm,
                        radius_mm,
                        residual_mm,
                    } => LivePaperPrimitive::Circle {
                        id: format!("{label}-{id}"),
                        center_mm: translate(center_mm),
                        radius_mm,
                        residual_mm,
                    },
                    DrawingPrimitive::Polyline {
                        id,
                        points_mm,
                        closed,
                        residual_mm,
                    } => LivePaperPrimitive::Polyline {
                        id: format!("{label}-{id}"),
                        points_mm: points_mm.into_iter().map(translate).collect(),
                        closed,
                        residual_mm,
                    },
                };
                primitives.push(LivePaperCandidate {
                    primitive,
                    needs_confirmation: false,
                });
            }
        }
    }
    // Nearby but distinct endpoints are an unresolved break, never a silent join.
    let endpoints = primitives
        .iter()
        .map(|p| open_endpoints(&p.primitive))
        .collect::<Vec<_>>();
    for i in 0..primitives.len() {
        for j in i + 1..primitives.len() {
            if endpoints[i].iter().any(|a| {
                endpoints[j].iter().any(|b| {
                    let distance = (a[0] - b[0]).hypot(a[1] - b[1]);
                    distance > 0.2 && distance < 4.0
                })
            }) {
                primitives[i].needs_confirmation = true;
                primitives[j].needs_confirmation = true;
                diagnostics.push(
                    "Nearby broken stroke endpoints need confirmation or editing before joining."
                        .into(),
                );
            }
        }
    }
    primitives.truncate(128);
    diagnostics.sort();
    diagnostics.dedup();
    let mut encoded = Cursor::new(Vec::new());
    DynamicImage::ImageLuma8(rectified)
        .write_to(&mut encoded, ImageFormat::Png)
        .map_err(|e| format!("Could not encode paper preview: {e}"))?;
    Ok(LivePaperObservation {
        preview_data_url: format!(
            "data:image/png;base64,{}",
            BASE64.encode(encoded.into_inner())
        ),
        width_mm: WIDTH_MM,
        height_mm: HEIGHT_MM,
        primitives,
        diagnostics,
    })
}

fn open_endpoints(primitive: &LivePaperPrimitive) -> Vec<[f64; 2]> {
    match primitive {
        LivePaperPrimitive::Line {
            start_mm, end_mm, ..
        } => vec![*start_mm, *end_mm],
        LivePaperPrimitive::Polyline {
            points_mm,
            closed: false,
            ..
        } => points_mm
            .first()
            .zip(points_mm.last())
            .map(|(a, b)| vec![*a, *b])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}
fn path_length(points: &[[f64; 2]]) -> f64 {
    points
        .windows(2)
        .map(|p| (p[0][0] - p[1][0]).hypot(p[0][1] - p[1][1]))
        .sum()
}

fn paper_projection(gray: &GrayImage) -> Result<Projection, String> {
    let view = ImageView::new(
        gray.as_raw(),
        gray.width() as usize,
        gray.height() as usize,
        gray.width() as usize,
    )?;
    let mut detector = Detector::builder()
        .with_family(TagFamily::ArUco4x4_50)
        .with_decimation(1)
        .with_threads(1)
        .build();
    let detections = detector
        .detect(&view, None, None, false)
        .map_err(|e| format!("Marker detection failed: {e}"))?
        .reassemble_owned();
    let mut from = [(0.0f32, 0.0f32); 4];
    let mut markers = [None, None, None, None];
    for detection in &detections {
        if detection.id > 3 {
            continue;
        }
        let id = detection.id as usize;
        if markers[id].is_some() {
            return Err(format!(
                "Duplicate marker {id}; use one complete Ecky template."
            ));
        }
        let corners = detection.corners.map(|p| (p[0] as f32, p[1] as f32));
        let square = Projection::from_control_points(
            [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
            corners,
        )
        .ok_or("Marker corners are degenerate; flatten the sheet.")?;
        // The projective center is the intersection of diagonals, not a corner average.
        from[id] = square * (0.5, 0.5);
        markers[id] = Some(corners);
    }
    if markers.iter().any(Option::is_none) {
        let missing = markers
            .iter()
            .enumerate()
            .filter_map(|(i, m)| m.is_none().then_some(i.to_string()))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!("Paper marker lost ({missing}). Keep all four printed markers visible; geometry updates paused."));
    }
    let to = MARKER_CENTERS_MM.map(|(x, y)| {
        (
            (x - 25.0) * PIXELS_PER_MM as f32,
            (y - 38.0) * PIXELS_PER_MM as f32,
        )
    });
    let projection = Projection::from_control_points(from, to)
        .ok_or("Paper markers are degenerate; flatten the sheet and move the camera back.")?;
    for corners in markers.into_iter().flatten() {
        let rectified = corners.map(|p| projection * p);
        for i in 0..4 {
            let a = rectified[i];
            let b = rectified[(i + 1) % 4];
            let length = (a.0 - b.0).hypot(a.1 - b.1) / PIXELS_PER_MM as f32;
            if !(13.0..=19.0).contains(&length) {
                return Err("Marker spacing does not match the Ecky template. Use one flat sheet printed at actual size.".into());
            }
        }
    }
    Ok(projection)
}
pub fn template_pdf() -> Result<Vec<u8>, String> {
    Ok(include_bytes!("../../assets/live-paper-template.pdf").to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(name: &str) -> DynamicImage {
        let bytes: &[u8] = match name {
            "primitives.png" => include_bytes!("../../tests/fixtures/live-paper/primitives.png"),
            "perspective-noise.png" => {
                include_bytes!("../../tests/fixtures/live-paper/perspective-noise.png")
            }
            "marker-loss.png" => include_bytes!("../../tests/fixtures/live-paper/marker-loss.png"),
            "junctions.png" => include_bytes!("../../tests/fixtures/live-paper/junctions.png"),
            "blank.png" => include_bytes!("../../tests/fixtures/live-paper/blank.png"),
            _ => panic!("Unknown fixture {name}"),
        };
        image::load_from_memory(bytes).unwrap()
    }
    #[test]
    fn given_printed_markers_and_pen_when_observed_then_recovers_metric_primitives() {
        for name in ["primitives.png", "perspective-noise.png"] {
            let result = recognize_live_paper(&fixture(name)).unwrap();
            assert_eq!((result.width_mm, result.height_mm), (160.0, 220.0));
            assert!(result.primitives.iter().any(|p| matches!(&p.primitive,LivePaperPrimitive::Line{start_mm,end_mm,..} if (start_mm[0].min(end_mm[0])-15.0).abs()<1.0 && (start_mm[0].max(end_mm[0])-85.0).abs()<1.0 && (start_mm[1]-180.0).abs()<1.0)), "{name}: {:?}",result.primitives);
            assert!(result.primitives.iter().any(|p| matches!(&p.primitive,LivePaperPrimitive::Circle{center_mm,radius_mm,..} if (center_mm[0]-115.0).abs()<1.0 && (center_mm[1]-140.0).abs()<1.0 && (radius_mm-12.0).abs()<0.8)),"{name}: {:?}",result.primitives);
            assert!(result.primitives.iter().any(|p| matches!(&p.primitive,LivePaperPrimitive::Polyline{points_mm,closed:true,..} if points_mm.len()>=4 && points_mm.len()<=8)),"{name}: {:?}",result.primitives);
            assert!(result.primitives.iter().any(|p| matches!(&p.primitive,LivePaperPrimitive::Point{position_mm,..} if (position_mm[0]-115.0).abs()<1.0 && (position_mm[1]-185.0).abs()<1.0)),"{name}: {:?}",result.primitives);
        }
    }
    #[test]
    fn given_marker_loss_when_observed_then_pauses_without_guessing_scale() {
        let error = recognize_live_paper(&fixture("marker-loss.png")).unwrap_err();
        assert!(error.contains("marker"), "{error}");
    }
    #[test]
    fn given_t_x_and_broken_strokes_when_observed_then_requires_confirmation() {
        let result = recognize_live_paper(&fixture("junctions.png")).unwrap();
        assert!(
            result
                .primitives
                .iter()
                .filter(|p| p.needs_confirmation)
                .count()
                >= 4,
            "{:?}",
            result.primitives
        );
        assert!(result.diagnostics.iter().any(|d| d.contains("junction")));
    }
    #[test]
    fn given_broken_stroke_when_observed_then_keeps_two_unjoined_proposals() {
        let result = recognize_live_paper(&fixture("junctions.png")).unwrap();
        let broken=result.primitives.iter().filter(|p|matches!(&p.primitive,LivePaperPrimitive::Line{start_mm,end_mm,..} if (start_mm[1]-100.0).abs()<1.0 && (end_mm[1]-100.0).abs()<1.0)).collect::<Vec<_>>();
        assert_eq!(broken.len(), 2);
        assert!(broken.iter().all(|p| p.needs_confirmation));
    }
    #[test]
    fn given_rotated_paper_when_observed_then_marker_identity_preserves_coordinates() {
        let image = fixture("primitives.png").rotate90();
        let result = recognize_live_paper(&image).unwrap();
        assert!(result.primitives.iter().any(|p|matches!(&p.primitive,LivePaperPrimitive::Circle{center_mm,radius_mm,..} if (center_mm[0]-115.0).abs()<1.0 && (center_mm[1]-140.0).abs()<1.0 && (radius_mm-12.0).abs()<0.8)));
    }
    #[test]
    fn given_dense_occlusion_when_observed_then_does_not_trace_hand_boundary() {
        let image = image::load_from_memory(include_bytes!(
            "../../tests/fixtures/live-paper/occlusion.png"
        ))
        .unwrap();
        let result = recognize_live_paper(&image).unwrap();
        assert!(result.diagnostics.iter().any(|d| d.contains("occlusion")));
        assert!(result
            .primitives
            .iter()
            .all(|p| matches!(p.primitive, LivePaperPrimitive::Point { .. })));
    }
    #[test]
    fn given_blank_paper_when_observed_then_returns_preview_without_invented_geometry() {
        let result = recognize_live_paper(&fixture("blank.png")).unwrap();
        assert!(result.primitives.is_empty());
        assert!(result
            .preview_data_url
            .starts_with("data:image/png;base64,"));
    }
}
