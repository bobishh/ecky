use crate::contracts::{AppError, AppResult};
use std::str::FromStr;
use usvg::tiny_skia_path::{PathSegment, Point};
use usvg::{self, Node, Transform, Tree, Visibility};

const EPS: f64 = 1e-9;
const CURVE_SAMPLES: usize = 12;
const QUAD_SAMPLES: usize = 8;

/// Exact contour segment (ocpsvg/build123d parity). Geometry destined for OCCT
/// keeps lines and cubic Béziers as-is; quadratics are elevated to cubics
/// losslessly. The flattened sample points remain only for Rust-side validity
/// checks (self-intersection, containment, fit bounds).
#[derive(Debug, Clone, PartialEq)]
pub enum SvgPathSegment {
    Line { to: [f64; 2] },
    Cubic { c1: [f64; 2], c2: [f64; 2], to: [f64; 2] },
}

/// Exact geometry of one closed contour: a start anchor plus consecutive
/// segments in authored order. Winding is not normalized here — the OCCT face
/// builders repair orientation downstream (ShapeFix_Face).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SvgContourGeometry {
    pub start: [f64; 2],
    pub segments: Vec<SvgPathSegment>,
}

impl SvgContourGeometry {
    pub fn has_curves(&self) -> bool {
        self.segments
            .iter()
            .any(|segment| matches!(segment, SvgPathSegment::Cubic { .. }))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SvgProfile {
    pub outer_loop: Vec<[f64; 2]>,
    pub hole_loops: Vec<Vec<[f64; 2]>>,
    /// Exact geometry parallel to `outer_loop` / `hole_loops`.
    pub outer_geometry: SvgContourGeometry,
    pub hole_geometries: Vec<SvgContourGeometry>,
    pub fit: SvgProfileFit,
    pub source_view_box: [f64; 4],
}

#[derive(Debug, Clone, PartialEq)]
pub struct SvgProfileFit {
    pub mode: SvgFitMode,
    pub target_width: Option<f64>,
    pub target_height: Option<f64>,
    pub scale_x: f64,
    pub scale_y: f64,
    pub translate_x: f64,
    pub translate_y: f64,
    pub source_width: f64,
    pub source_height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvgFitMode {
    Contain,
    Cover,
    Stretch,
}

impl FromStr for SvgFitMode {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "contain" | "fit" => Ok(Self::Contain),
            "cover" => Ok(Self::Cover),
            "stretch" | "fill" => Ok(Self::Stretch),
            _ => Err(()),
        }
    }
}

/// One extracted contour: flattened check-points plus exact geometry.
#[derive(Debug, Clone)]
struct ExtractedContour {
    points: Vec<[f64; 2]>,
    geometry: SvgContourGeometry,
}

#[derive(Debug, Clone, Default)]
struct ProfileParseState {
    loops: Vec<ExtractedContour>,
    has_visible_path: bool,
    has_raster_or_text: bool,
}

/// SVG fill-rule as authored on a `<path>`. Determines how OCCT resolves nested
/// wires into filled regions downstream (mirrors ocpsvg's per-path handling).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvgFillRule {
    NonZero,
    EvenOdd,
}

/// One `<path>`'s tolerant wire extraction: every closed wire it produced plus
/// the fill-rule that governs how those wires nest into regions. Region
/// resolution (containment/parity) is delegated to OCCT, so this stage keeps all
/// wires without rejecting self-intersecting, open, or multi-outer geometry.
#[derive(Debug, Clone)]
pub struct SvgWireSoup {
    pub fill_rule: SvgFillRule,
    pub wires: Vec<Vec<[f64; 2]>>,
    /// Exact geometry parallel to `wires` (same indices).
    pub wire_geometries: Vec<SvgContourGeometry>,
}

/// Tolerant per-`<path>` wire-soup extraction. Unlike [`parse_svg_profile`] (the
/// clean fast path), this never rejects self-intersecting / open / multi-outer
/// contours: it collects the raw closed wires and captures each path's fill-rule
/// so a downstream OCCT face builder can resolve regions. Degenerate wires
/// (fewer than three points, near-zero area) are silently dropped.
pub fn extract_svg_wire_soup(svg_text: &str) -> AppResult<Vec<SvgWireSoup>> {
    let fontdb = usvg::fontdb::Database::new();
    let tree = Tree::from_str(svg_text, &usvg::Options::default(), &fontdb)
        .map_err(|err| AppError::validation(err.to_string()))?;

    let mut soups = Vec::new();
    collect_wire_soup(tree.root(), &mut soups)?;
    Ok(soups)
}

/// Fit-transformed wire soup ready to hand to the OCCT planar-face builder for
/// region resolution. All wires share one fit computed over their combined
/// bounds (so a compound of disjoint shapes keeps its relative layout), and the
/// fill-rule collapses to even-odd if any contributing `<path>` used it.
#[derive(Debug, Clone)]
pub struct SvgWireSoupProfile {
    pub wires: Vec<Vec<[f64; 2]>>,
    /// Exact geometry parallel to `wires` (same indices), fit-transformed.
    pub wire_geometries: Vec<SvgContourGeometry>,
    pub fill_rule: SvgFillRule,
}

/// Tolerant sibling of [`parse_svg_profile`]: extract every filled `<path>`'s
/// wires (no self-intersection / single-outer / closed-only rejection), apply
/// the same fit transform, and return the raw soup for OCCT to resolve into
/// faces. Errors only when there is no faceable filled geometry at all.
pub fn extract_svg_wire_soup_profile(
    svg_text: &str,
    target_width: Option<f64>,
    target_height: Option<f64>,
    fit_mode: SvgFitMode,
) -> AppResult<SvgWireSoupProfile> {
    let soups = extract_svg_wire_soup(svg_text)?;

    let mut wires: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut wire_geometries: Vec<SvgContourGeometry> = Vec::new();
    let mut any_even_odd = false;
    for soup in &soups {
        if soup.fill_rule == SvgFillRule::EvenOdd {
            any_even_odd = true;
        }
        wires.extend(soup.wires.iter().cloned());
        wire_geometries.extend(soup.wire_geometries.iter().cloned());
    }

    if wires.is_empty() {
        return Err(AppError::validation(
            "SVG has no faceable filled paths for native region resolution. \
             Stroke-only lineart is not yet supported by the native backend.",
        ));
    }

    // compute_fit only reads the combined point set for bounds/center; the
    // outer/holes split is irrelevant here, so treat the first wire as outer.
    let (outer, holes) = wires.split_first().expect("wires non-empty");
    let holes_vec: Vec<Vec<[f64; 2]>> = holes.to_vec();
    let fit = compute_fit(outer, &holes_vec, target_width, target_height, fit_mode)?;

    let wires = wires
        .into_iter()
        .map(|points| transform_loop(points, &fit))
        .collect();
    let wire_geometries = wire_geometries
        .into_iter()
        .map(|geometry| transform_geometry(geometry, &fit))
        .collect();

    Ok(SvgWireSoupProfile {
        wires,
        wire_geometries,
        fill_rule: if any_even_odd {
            SvgFillRule::EvenOdd
        } else {
            SvgFillRule::NonZero
        },
    })
}

fn collect_wire_soup(root: &usvg::Group, soups: &mut Vec<SvgWireSoup>) -> AppResult<()> {
    for node in root.children() {
        match node {
            Node::Group(group) => collect_wire_soup(group, soups)?,
            Node::Path(path) => {
                if path.visibility() != Visibility::Visible {
                    continue;
                }
                let fill = match path.fill() {
                    Some(fill) => fill,
                    // No fill: stroke-only paths carry no region intent for the
                    // face builder, so they contribute no wire soup.
                    None => continue,
                };
                let fill_rule = match fill.rule() {
                    usvg::FillRule::NonZero => SvgFillRule::NonZero,
                    usvg::FillRule::EvenOdd => SvgFillRule::EvenOdd,
                };
                let mut contours = Vec::new();
                extract_contours(
                    path.data(),
                    path.abs_transform(),
                    path.id(),
                    &mut contours,
                    true,
                )?;
                if !contours.is_empty() {
                    let mut wires = Vec::with_capacity(contours.len());
                    let mut wire_geometries = Vec::with_capacity(contours.len());
                    for contour in contours {
                        wires.push(contour.points);
                        wire_geometries.push(contour.geometry);
                    }
                    soups.push(SvgWireSoup {
                        fill_rule,
                        wires,
                        wire_geometries,
                    });
                }
            }
            Node::Image(_) | Node::Text(_) => {}
        }
    }
    Ok(())
}

pub fn parse_svg_profile(
    svg_text: &str,
    target_width: Option<f64>,
    target_height: Option<f64>,
    fit_mode: SvgFitMode,
    reject_multi_outer_first_slice: bool,
) -> AppResult<SvgProfile> {
    let fontdb = usvg::fontdb::Database::new();
    let tree = Tree::from_str(svg_text, &usvg::Options::default(), &fontdb)
        .map_err(|err| AppError::validation(err.to_string()))?;

    let mut state = ProfileParseState::default();
    collect_visible_loops(tree.root(), &mut state)?;

    if !state.has_visible_path {
        if state.has_raster_or_text {
            return Err(AppError::validation(
                "SVG contains raster/text content without visible vector paths; raster-only SVG is unsupported.",
            ));
        }
        return Err(AppError::validation(
            "SVG profile ingestion requires at least one visible vector path.",
        ));
    }

    let normalized = normalize_loops(state.loops)?;
    if normalized.is_empty() {
        return Err(AppError::validation(
            "SVG profile ingestion found no closed loops.",
        ));
    }

    // The clean fast path assumes properly nested, disjoint loops. Loops that
    // cross EACH OTHER (banana-lineart style artwork: every stroke outline
    // passed the per-contour self-intersection check, then classified as
    // "holes" of the biggest outline) build faces with intersecting rings —
    // OCCT garbage downstream. Reject here so the wire-soup resolver, which
    // splits intersections properly, takes over.
    if loops_mutually_intersect(&normalized) {
        return Err(AppError::validation(
            "SVG contours intersect each other; artwork requires wire-soup region resolution.",
        ));
    }

    let (outer, holes) = classify_loops(normalized, reject_multi_outer_first_slice)?;
    let hole_loops: Vec<Vec<[f64; 2]>> =
        holes.iter().map(|contour| contour.points.clone()).collect();
    let fit = compute_fit(
        &outer.points,
        &hole_loops,
        target_width,
        target_height,
        fit_mode,
    )?;

    let source_view_box = tree.view_box();
    Ok(SvgProfile {
        outer_loop: transform_loop(outer.points, &fit),
        hole_loops: hole_loops
            .into_iter()
            .map(|points| transform_loop(points, &fit))
            .collect(),
        outer_geometry: transform_geometry(outer.geometry, &fit),
        hole_geometries: holes
            .into_iter()
            .map(|contour| transform_geometry(contour.geometry, &fit))
            .collect(),
        fit,
        source_view_box: [
            f64::from(source_view_box.rect.x()),
            f64::from(source_view_box.rect.y()),
            f64::from(source_view_box.rect.width()),
            f64::from(source_view_box.rect.height()),
        ],
    })
}

fn collect_visible_loops(root: &usvg::Group, state: &mut ProfileParseState) -> AppResult<()> {
    for node in root.children() {
        collect_visible_node(node, state)?;
    }

    Ok(())
}

fn collect_visible_node(node: &Node, state: &mut ProfileParseState) -> AppResult<()> {
    match node {
        Node::Group(group) => collect_visible_loops(group, state),
        Node::Path(path) => {
            if path.visibility() != Visibility::Visible {
                return Ok(());
            }
            let visible = path.fill().is_some() || path.stroke().is_some();
            if !visible {
                return Ok(());
            }
            state.has_visible_path = true;
            extract_contours(
                path.data(),
                path.abs_transform(),
                path.id(),
                &mut state.loops,
                false,
            )?;
            Ok(())
        }
        Node::Image(_) | Node::Text(_) => {
            state.has_raster_or_text = true;
            Ok(())
        }
    }
}

fn extract_contours(
    path_data: &usvg::tiny_skia_path::Path,
    abs_transform: Transform,
    _context: &str,
    contours: &mut Vec<ExtractedContour>,
    tolerant: bool,
) -> AppResult<()> {
    let mut current: Vec<[f64; 2]> = Vec::new();
    let mut geometry = SvgContourGeometry::default();
    let mut start: Option<[f64; 2]> = None;

    let mut finalize = |current: &mut Vec<[f64; 2]>,
                        geometry: &mut SvgContourGeometry,
                        start: &mut Option<[f64; 2]>,
                        close_explicit: bool|
     -> AppResult<()> {
        let contour_geometry = std::mem::take(geometry);
        if current.len() < 2 {
            current.clear();
            *start = None;
            return Ok(());
        }

        let loop_points = current.clone();
        *current = Vec::new();
        let mut cleaned = match normalize_contour(loop_points) {
            Ok(value) => value,
            Err(_) if tolerant => {
                let _ = start.take();
                return Ok(());
            }
            Err(err) => return Err(err),
        };

        if cleaned.len() < 3 {
            if tolerant {
                let _ = start.take();
                return Ok(());
            }
            return Err(AppError::validation(
                "SVG path contour has fewer than three distinct points.",
            ));
        }

        if !close_explicit && !points_equal(cleaned[0], cleaned[cleaned.len() - 1]) {
            if tolerant {
                let _ = start.take();
                return Ok(());
            }
            return Err(AppError::validation(
                "SVG path contour is open. Closed paths are required for profile ingestion.",
            ));
        }

        if close_explicit && !points_equal(cleaned[0], cleaned[cleaned.len() - 1]) {
            cleaned.push(cleaned[0]);
        }

        if points_equal(cleaned[0], cleaned[cleaned.len() - 1]) {
            cleaned.pop();
        }

        if cleaned.len() < 3 {
            if tolerant {
                let _ = start.take();
                return Ok(());
            }
            return Err(AppError::validation(
                "SVG path contour has fewer than three points.",
            ));
        }

        if !tolerant && self_intersects(&cleaned) {
            return Err(AppError::validation(
                "SVG path contour is self-intersecting and cannot be used as profile geometry.",
            ));
        }
        // tolerant mode intentionally skips the self-intersection check: region
        // resolution is delegated to OCCT's planar face builder downstream
        // (BRepBuilderAPI_MakeFace), mirroring the build123d/ocpsvg path.

        if signed_area(&cleaned).abs() < EPS {
            if tolerant {
                let _ = start.take();
                return Ok(());
            }
            return Err(AppError::validation("SVG path contour has near-zero area."));
        }

        let _ = start.take();
        contours.push(ExtractedContour {
            points: cleaned,
            geometry: contour_geometry,
        });
        Ok(())
    };

    for segment in path_data.segments() {
        match segment {
            PathSegment::MoveTo(point) => {
                if !current.is_empty() {
                    finalize(&mut current, &mut geometry, &mut start, false)?;
                }
                let point = transform_point(point, abs_transform);
                current.push(point);
                geometry = SvgContourGeometry {
                    start: point,
                    segments: Vec::new(),
                };
                start = Some(point);
            }
            PathSegment::LineTo(point) => {
                if start.is_none() {
                    return Err(AppError::validation(
                        "SVG path contour missing a starting point.",
                    ));
                }
                let point = transform_point(point, abs_transform);
                if current.is_empty() || !points_equal(*current.last().expect("non-empty"), point) {
                    current.push(point);
                    geometry.segments.push(SvgPathSegment::Line { to: point });
                }
            }
            PathSegment::QuadTo(c1, c2) => {
                if start.is_none() || current.is_empty() {
                    return Err(AppError::validation(
                        "SVG path contour missing a starting point.",
                    ));
                }
                let anchor = *current.last().expect("non-empty");
                geometry
                    .segments
                    .push(quad_to_cubic(anchor, c1, c2, abs_transform));
                sample_quad(anchor, c1, c2, abs_transform, &mut current);
            }
            PathSegment::CubicTo(c1, c2, c3) => {
                if start.is_none() || current.is_empty() {
                    return Err(AppError::validation(
                        "SVG path contour missing a starting point.",
                    ));
                }
                let anchor = *current.last().expect("non-empty");
                geometry.segments.push(SvgPathSegment::Cubic {
                    c1: transform_point(c1, abs_transform),
                    c2: transform_point(c2, abs_transform),
                    to: transform_point(c3, abs_transform),
                });
                sample_cubic(anchor, c1, c2, c3, abs_transform, &mut current);
            }
            PathSegment::Close => {
                if start.is_none() {
                    continue;
                }
                finalize(&mut current, &mut geometry, &mut start, true)?;
            }
        }
    }

    if !current.is_empty() {
        finalize(&mut current, &mut geometry, &mut start, false)?;
    }

    Ok(())
}

/// Lossless degree elevation of a quadratic Bézier to a cubic:
/// c1 = q0 + ⅔(q1 − q0), c2 = q2 + ⅔(q1 − q2).
fn quad_to_cubic(anchor: [f64; 2], c1: Point, c2: Point, transform: Transform) -> SvgPathSegment {
    let q1 = transform_point(c1, transform);
    let q2 = transform_point(c2, transform);
    SvgPathSegment::Cubic {
        c1: [
            anchor[0] + 2.0 / 3.0 * (q1[0] - anchor[0]),
            anchor[1] + 2.0 / 3.0 * (q1[1] - anchor[1]),
        ],
        c2: [
            q2[0] + 2.0 / 3.0 * (q1[0] - q2[0]),
            q2[1] + 2.0 / 3.0 * (q1[1] - q2[1]),
        ],
        to: q2,
    }
}

fn transform_point(point: Point, transform: Transform) -> [f64; 2] {
    let mut point = point;
    transform.map_point(&mut point);
    [f64::from(point.x), f64::from(point.y)]
}

fn sample_quad(
    anchor: [f64; 2],
    c1: Point,
    c2: Point,
    transform: Transform,
    out: &mut Vec<[f64; 2]>,
) {
    let c1 = transform_point(c1, transform);
    let c2 = transform_point(c2, transform);

    for step in 1..=QUAD_SAMPLES {
        let t = (step as f64) / (QUAD_SAMPLES as f64);
        let omt = 1.0 - t;
        let point = [
            omt * omt * anchor[0] + 2.0 * omt * t * c1[0] + t * t * c2[0],
            omt * omt * anchor[1] + 2.0 * omt * t * c1[1] + t * t * c2[1],
        ];
        if !out.is_empty() && points_equal(*out.last().expect("non-empty"), point) {
            continue;
        }
        out.push(point);
    }
}

fn sample_cubic(
    anchor: [f64; 2],
    c1: Point,
    c2: Point,
    c3: Point,
    transform: Transform,
    out: &mut Vec<[f64; 2]>,
) {
    let c1 = transform_point(c1, transform);
    let c2 = transform_point(c2, transform);
    let c3 = transform_point(c3, transform);

    for step in 1..=CURVE_SAMPLES {
        let t = (step as f64) / (CURVE_SAMPLES as f64);
        let omt = 1.0 - t;
        let omt2 = omt * omt;
        let t2 = t * t;
        let point = [
            omt2 * omt * anchor[0]
                + 3.0 * omt2 * t * c1[0]
                + 3.0 * omt * t2 * c2[0]
                + t2 * t * c3[0],
            omt2 * omt * anchor[1]
                + 3.0 * omt2 * t * c1[1]
                + 3.0 * omt * t2 * c2[1]
                + t2 * t * c3[1],
        ];
        if !out.is_empty() && points_equal(*out.last().expect("non-empty"), point) {
            continue;
        }
        out.push(point);
    }
}

fn normalize_contour(mut contour: Vec<[f64; 2]>) -> AppResult<Vec<[f64; 2]>> {
    if contour.len() < 3 {
        return Err(AppError::validation("SVG path contour has too few points."));
    }

    contour = deduplicate_points(contour);
    if contour.len() < 3 {
        return Err(AppError::validation(
            "SVG path contour has fewer than three distinct points.",
        ));
    }

    if points_equal(contour[0], contour[contour.len() - 1]) {
        contour.pop();
    }
    if contour.len() < 3 {
        return Err(AppError::validation("SVG path contour has too few points."));
    }
    Ok(contour)
}

fn normalize_loops(contours: Vec<ExtractedContour>) -> AppResult<Vec<ExtractedContour>> {
    let mut normalized = Vec::with_capacity(contours.len());
    for contour in contours {
        let mut points = normalize_contour(contour.points)?;
        if points.len() < 3 {
            return Err(AppError::validation(
                "SVG path contour has fewer than three usable points after normalization.",
            ));
        }
        if self_intersects(&points) {
            return Err(AppError::validation(
                "SVG path contour is self-intersecting and cannot be used as profile geometry.",
            ));
        }
        if signed_area(&points).abs() < EPS {
            return Err(AppError::validation("SVG path contour has near-zero area."));
        }

        // Only the flattened check-points get winding-normalized; the exact
        // geometry keeps authored order (OCCT's ShapeFix repairs orientation).
        if signed_area(&points) < 0.0 {
            points.reverse();
        }
        normalized.push(ExtractedContour {
            points,
            geometry: contour.geometry,
        });
    }
    Ok(normalized)
}

fn classify_loops(
    loops: Vec<ExtractedContour>,
    reject_multi_outer_first_slice: bool,
) -> AppResult<(ExtractedContour, Vec<ExtractedContour>)> {
    let mut parent: Vec<Option<usize>> = vec![None; loops.len()];

    for i in 0..loops.len() {
        let rep = polygon_representative_point(&loops[i].points);
        let mut parent_area = f64::INFINITY;
        let mut parent_index = None;

        for j in 0..loops.len() {
            if i == j {
                continue;
            }
            let candidate_area = signed_area(&loops[j].points).abs();
            if candidate_area <= signed_area(&loops[i].points).abs() {
                continue;
            }
            if point_in_polygon(rep, &loops[j].points) && candidate_area < parent_area {
                parent_area = candidate_area;
                parent_index = Some(j);
            }
        }

        parent[i] = parent_index;
    }

    let outer_indices: Vec<usize> = parent
        .iter()
        .enumerate()
        .filter_map(|(index, parent_index)| parent_index.is_none().then_some(index))
        .collect();

    if outer_indices.is_empty() {
        return Err(AppError::validation(
            "SVG profile ingestion found no outer loops.",
        ));
    }
    if reject_multi_outer_first_slice && outer_indices.len() > 1 {
        return Err(AppError::validation(
            "SVG profile ingestion found multiple outer loops; this slice only supports one outer loop.",
        ));
    }

    let outer_index = outer_indices
        .into_iter()
        .max_by(|left, right| {
            signed_area(&loops[*left].points)
                .abs()
                .total_cmp(&signed_area(&loops[*right].points).abs())
        })
        .ok_or_else(|| AppError::validation("SVG profile ingestion found no outer loops."))?;

    let mut holes = Vec::new();
    for (index, contour) in loops.iter().enumerate() {
        if index == outer_index {
            continue;
        }
        if is_descendant(index, outer_index, &parent) {
            let mut hole = contour.clone();
            if signed_area(&hole.points) > 0.0 {
                hole.points.reverse();
            }
            holes.push(hole);
        }
    }

    let mut outer = loops[outer_index].clone();
    if signed_area(&outer.points) < 0.0 {
        outer.points.reverse();
    }
    Ok((outer, holes))
}

fn is_descendant(mut index: usize, target: usize, parent: &[Option<usize>]) -> bool {
    while let Some(next) = parent[index] {
        if next == target {
            return true;
        }
        if next == index {
            return false;
        }
        index = next;
    }
    false
}

fn compute_fit(
    outer_loop: &[[f64; 2]],
    holes: &[Vec<[f64; 2]>],
    target_width: Option<f64>,
    target_height: Option<f64>,
    fit_mode: SvgFitMode,
) -> AppResult<SvgProfileFit> {
    let mut all_points = outer_loop.to_vec();
    for points in holes {
        all_points.extend(points.iter().copied());
    }
    let source_bounds = Bounds2d::from_points(&all_points)?;
    let source_width = source_bounds.width();
    let source_height = source_bounds.height();

    if target_width.is_none() && target_height.is_none() {
        return Ok(SvgProfileFit {
            mode: fit_mode,
            target_width,
            target_height,
            scale_x: 1.0,
            scale_y: 1.0,
            translate_x: 0.0,
            translate_y: 0.0,
            source_width,
            source_height,
        });
    }

    let (requested_scale_x, requested_scale_y) = match (target_width, target_height) {
        (Some(width), Some(height)) => {
            let scale_x = validate_positive(width, "target_width")? / source_width;
            let scale_y = validate_positive(height, "target_height")? / source_height;
            (Some(scale_x), Some(scale_y))
        }
        (Some(width), None) => {
            let uniform = validate_positive(width, "target_width")? / source_width;
            (Some(uniform), None)
        }
        (None, Some(height)) => {
            let uniform = validate_positive(height, "target_height")? / source_height;
            (Some(uniform), Some(uniform))
        }
        (None, None) => (None, None),
    };

    let (scale_x, scale_y) = match fit_mode {
        SvgFitMode::Stretch => {
            if let (Some(sx), Some(sy)) = (requested_scale_x, requested_scale_y) {
                (sx, sy)
            } else {
                let uniform = requested_scale_x.or(requested_scale_y).unwrap_or(1.0);
                (uniform, uniform)
            }
        }
        SvgFitMode::Contain => {
            let scale = requested_scale_x
                .and_then(|sx| requested_scale_y.map(|sy| sx.min(sy)))
                .or(requested_scale_x)
                .or(requested_scale_y)
                .unwrap_or(1.0);
            (scale, scale)
        }
        SvgFitMode::Cover => {
            let scale = requested_scale_x
                .and_then(|sx| requested_scale_y.map(|sy| sx.max(sy)))
                .or(requested_scale_x)
                .or(requested_scale_y)
                .unwrap_or(1.0);
            (scale, scale)
        }
    };

    if !scale_x.is_finite() || !scale_y.is_finite() || scale_x <= 0.0 || scale_y <= 0.0 {
        return Err(AppError::validation(
            "Computed SVG fit scale must be positive and finite.",
        ));
    }

    Ok(SvgProfileFit {
        mode: fit_mode,
        target_width,
        target_height,
        scale_x,
        scale_y,
        translate_x: -source_bounds.center_x(),
        translate_y: -source_bounds.center_y(),
        source_width,
        source_height,
    })
}

fn validate_positive(value: f64, field: &str) -> AppResult<f64> {
    if !value.is_finite() || value <= 0.0 {
        return Err(AppError::validation(format!(
            "SVG fit target {} must be a positive finite number.",
            field
        )));
    }
    Ok(value)
}

fn transform_loop(points: Vec<[f64; 2]>, fit: &SvgProfileFit) -> Vec<[f64; 2]> {
    points
        .into_iter()
        .map(|point| fit_point(point, fit))
        .collect()
}

fn fit_point(point: [f64; 2], fit: &SvgProfileFit) -> [f64; 2] {
    [
        (point[0] + fit.translate_x) * fit.scale_x,
        (point[1] + fit.translate_y) * fit.scale_y,
    ]
}

/// The fit is affine, so it maps Bézier control points exactly.
fn transform_geometry(geometry: SvgContourGeometry, fit: &SvgProfileFit) -> SvgContourGeometry {
    SvgContourGeometry {
        start: fit_point(geometry.start, fit),
        segments: geometry
            .segments
            .into_iter()
            .map(|segment| match segment {
                SvgPathSegment::Line { to } => SvgPathSegment::Line {
                    to: fit_point(to, fit),
                },
                SvgPathSegment::Cubic { c1, c2, to } => SvgPathSegment::Cubic {
                    c1: fit_point(c1, fit),
                    c2: fit_point(c2, fit),
                    to: fit_point(to, fit),
                },
            })
            .collect(),
    }
}

fn deduplicate_points(points: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    let mut deduped = Vec::new();
    for point in points {
        if deduped
            .last()
            .is_none_or(|last| !points_equal(*last, point))
        {
            deduped.push(point);
        }
    }
    if deduped.len() > 1
        && points_equal(
            *deduped.first().expect("first"),
            *deduped.last().expect("last"),
        )
    {
        deduped.pop();
    }
    deduped
}

fn points_equal(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() <= EPS && (a[1] - b[1]).abs() <= EPS
}

fn point_in_polygon(point: [f64; 2], polygon: &[[f64; 2]]) -> bool {
    let mut inside = false;
    let (px, py) = (point[0], point[1]);
    for i in 0..polygon.len() {
        let j = (i + 1) % polygon.len();
        let (xi, yi) = (polygon[i][0], polygon[i][1]);
        let (xj, yj) = (polygon[j][0], polygon[j][1]);
        let intersects =
            ((yi > py) != (yj > py)) && (px < ((xj - xi) * (py - yi) / (yj - yi) + xi));
        if intersects {
            inside = !inside;
        }
    }
    inside
}

fn polygon_representative_point(points: &[[f64; 2]]) -> [f64; 2] {
    let centroid = signed_area_centroid(points);
    if point_in_polygon(centroid, points) {
        centroid
    } else {
        let mut min_x = points[0][0];
        let mut max_x = points[0][0];
        let mut min_y = points[0][1];
        let mut max_y = points[0][1];
        for point in points {
            min_x = min_x.min(point[0]);
            max_x = max_x.max(point[0]);
            min_y = min_y.min(point[1]);
            max_y = max_y.max(point[1]);
        }
        let bbox_center = [(min_x + max_x) * 0.5, (min_y + max_y) * 0.5];
        if point_in_polygon(bbox_center, points) {
            bbox_center
        } else {
            points[0]
        }
    }
}

fn signed_area_centroid(points: &[[f64; 2]]) -> [f64; 2] {
    let mut cx = 0.0;
    let mut cy = 0.0;
    let mut area_sum = 0.0;
    for i in 0..points.len() {
        let j = (i + 1) % points.len();
        let cross = points[i][0] * points[j][1] - points[j][0] * points[i][1];
        area_sum += cross;
        cx += (points[i][0] + points[j][0]) * cross;
        cy += (points[i][1] + points[j][1]) * cross;
    }
    if area_sum.abs() < EPS {
        return points[0];
    }
    let factor = 1.0 / (3.0 * area_sum);
    [cx * factor, cy * factor]
}

fn signed_area(points: &[[f64; 2]]) -> f64 {
    let mut area = 0.0;
    for i in 0..points.len() {
        let j = (i + 1) % points.len();
        area += points[i][0] * points[j][1];
        area -= points[j][0] * points[i][1];
    }
    0.5 * area
}

/// True when any two distinct loops cross each other (segment-level test on
/// the flattened check-points).
fn loops_mutually_intersect(loops: &[ExtractedContour]) -> bool {
    for left_index in 0..loops.len() {
        for right_index in (left_index + 1)..loops.len() {
            let left = &loops[left_index].points;
            let right = &loops[right_index].points;
            for i in 0..left.len() {
                let a1 = left[i];
                let a2 = left[(i + 1) % left.len()];
                for j in 0..right.len() {
                    let b1 = right[j];
                    let b2 = right[(j + 1) % right.len()];
                    if segments_intersect(a1, a2, b1, b2) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn self_intersects(points: &[[f64; 2]]) -> bool {
    if points.len() < 4 {
        return false;
    }
    let mut prev = 0;
    for i in 0..points.len() {
        let i_next = (i + 1) % points.len();
        for j in (i + 1)..points.len() {
            let j_next = (j + 1) % points.len();

            if i == j {
                continue;
            }
            if i_next == j || j_next == i {
                continue;
            }
            if i == 0 && j_next == i {
                continue;
            }

            if segments_intersect(points[i], points[i_next], points[j], points[j_next]) {
                return true;
            }
        }
        prev = i;
    }
    let _ = prev;
    false
}

fn segments_intersect(a1: [f64; 2], a2: [f64; 2], b1: [f64; 2], b2: [f64; 2]) -> bool {
    fn cross(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    }

    fn on_segment(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> bool {
        let min_x = a[0].min(b[0]) - EPS;
        let max_x = a[0].max(b[0]) + EPS;
        let min_y = a[1].min(b[1]) - EPS;
        let max_y = a[1].max(b[1]) + EPS;
        cross(a, b, c).abs() <= EPS
            && c[0] >= min_x
            && c[0] <= max_x
            && c[1] >= min_y
            && c[1] <= max_y
    }

    let d1 = cross(a1, a2, b1);
    let d2 = cross(a1, a2, b2);
    let d3 = cross(b1, b2, a1);
    let d4 = cross(b1, b2, a2);

    if ((d1 > EPS && d2 < -EPS) || (d1 < -EPS && d2 > EPS))
        && ((d3 > EPS && d4 < -EPS) || (d3 < -EPS && d4 > EPS))
    {
        return true;
    }

    if d1.abs() <= EPS && on_segment(a1, a2, b1) {
        return true;
    }
    if d2.abs() <= EPS && on_segment(a1, a2, b2) {
        return true;
    }
    if d3.abs() <= EPS && on_segment(b1, b2, a1) {
        return true;
    }
    if d4.abs() <= EPS && on_segment(b1, b2, a2) {
        return true;
    }

    false
}

#[derive(Debug, Clone, Copy)]
struct Bounds2d {
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
}

impl Bounds2d {
    fn from_points(points: &[[f64; 2]]) -> AppResult<Self> {
        if points.is_empty() {
            return Err(AppError::validation(
                "SVG profile ingestion failed while computing source bounds: no points available.",
            ));
        }

        let mut bounds = Self {
            min_x: points[0][0],
            max_x: points[0][0],
            min_y: points[0][1],
            max_y: points[0][1],
        };

        for point in points {
            bounds.min_x = bounds.min_x.min(point[0]);
            bounds.max_x = bounds.max_x.max(point[0]);
            bounds.min_y = bounds.min_y.min(point[1]);
            bounds.max_y = bounds.max_y.max(point[1]);
        }

        if !(bounds.width().is_finite() && bounds.height().is_finite()) {
            return Err(AppError::validation(
                "SVG profile ingestion found non-finite source bounds.",
            ));
        }

        if bounds.width() <= 0.0 || bounds.height() <= 0.0 {
            return Err(AppError::validation(
                "SVG profile geometry has zero width or height after normalization.",
            ));
        }

        Ok(bounds)
    }

    fn width(self) -> f64 {
        self.max_x - self.min_x
    }

    fn height(self) -> f64 {
        self.max_y - self.min_y
    }

    fn center_x(self) -> f64 {
        (self.min_x + self.max_x) * 0.5
    }

    fn center_y(self) -> f64 {
        (self.min_y + self.max_y) * 0.5
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_outer_loop_found() {
        let svg = r#"
            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
              <path d="M1 1 L9 1 L9 9 L1 9 Z"/>
            </svg>
        "#;
        let profile = parse_svg_profile(svg, Some(10.0), Some(10.0), SvgFitMode::Contain, true)
            .expect("outer loop should parse");
        assert_eq!(profile.outer_loop.len(), 4);
        assert!(profile.hole_loops.is_empty());
        assert!((profile.fit.scale_x - profile.fit.scale_y).abs() < 1e-9);
        assert!((profile.fit.scale_x - 1.0).abs() > 1e-9);
    }

    #[test]
    fn parse_hole_and_outer() {
        let svg = r#"
            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20">
              <path d="M1 1 L19 1 L19 19 L1 19 Z"/>
              <path d="M6 6 L6 14 L14 14 L14 6 Z"/>
            </svg>
        "#;
        let profile = parse_svg_profile(svg, Some(10.0), Some(10.0), SvgFitMode::Contain, true)
            .expect("outer and hole parse");
        assert_eq!(profile.outer_loop.len(), 4);
        assert_eq!(profile.hole_loops.len(), 1);
        assert!(signed_area(&profile.outer_loop) > 0.0);
        assert!(signed_area(&profile.hole_loops[0]) < 0.0);
    }

    #[test]
    fn parse_contains_viewbox_fit() {
        let svg = r#"
            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 2 1">
              <path d="M0 0 L2 0 L2 1 L0 1 Z"/>
            </svg>
        "#;
        let profile = parse_svg_profile(svg, Some(8.0), Some(5.0), SvgFitMode::Contain, true)
            .expect("fit should parse");
        let xs: Vec<f64> = profile.outer_loop.iter().map(|point| point[0]).collect();
        let ys: Vec<f64> = profile.outer_loop.iter().map(|point| point[1]).collect();
        let width = xs.iter().copied().max_by(f64::total_cmp).unwrap()
            - xs.iter().copied().min_by(f64::total_cmp).unwrap();
        let height = ys.iter().copied().max_by(f64::total_cmp).unwrap()
            - ys.iter().copied().min_by(f64::total_cmp).unwrap();
        assert!((width - 8.0).abs() < 1e-9);
        assert!((height - 4.0).abs() < 1e-9);
        assert!((profile.fit.scale_x - profile.fit.scale_y).abs() < 1e-9);
    }

    #[test]
    fn reject_raster_only_svg() {
        let svg = r#"
            <svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">
              <image x="0" y="0" width="4" height="4" xlink:href="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/wcAAwAB/9tN0wAAAABJRU5ErkJggg=="/>
            </svg>
        "#;
        let err = parse_svg_profile(svg, None, None, SvgFitMode::Contain, true)
            .expect_err("raster-only svg should fail");
        assert!(err
            .to_string()
            .contains("raster/text content without visible vector paths"));
    }

    #[test]
    fn reject_open_contour() {
        let svg = r#"
            <svg xmlns="http://www.w3.org/2000/svg">
              <path d="M1 1 L9 1 L9 9 L1 9"/>
            </svg>
        "#;
        let err = parse_svg_profile(svg, None, None, SvgFitMode::Contain, true)
            .expect_err("open contour should fail");
        assert!(err.to_string().contains("open"));
    }

    #[test]
    fn reject_self_intersection() {
        let svg = r#"
            <svg xmlns="http://www.w3.org/2000/svg">
              <path d="M0 0 L10 10 L0 10 L10 0 Z"/>
            </svg>
        "#;
        let err = parse_svg_profile(svg, None, None, SvgFitMode::Contain, true)
            .expect_err("self intersecting contour should fail");
        assert!(err.to_string().contains("self-intersecting"));
    }

    #[test]
    fn wire_soup_keeps_self_intersecting_lineart() {
        // A 5-point star self-intersects: the clean fast path rejects it, but the
        // tolerant wire-soup path keeps the wire for downstream OCCT resolution.
        let svg = r#"
            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
              <path d="M50 0 L20 90 L95 35 L5 35 L80 90 Z"/>
            </svg>
        "#;
        parse_svg_profile(svg, None, None, SvgFitMode::Contain, true)
            .expect_err("clean path rejects self-intersecting star");

        let soups = extract_svg_wire_soup(svg).expect("wire soup extracts");
        assert_eq!(soups.len(), 1, "one <path> => one soup");
        assert_eq!(soups[0].fill_rule, SvgFillRule::NonZero);
        assert_eq!(soups[0].wires.len(), 1, "star is a single closed wire");
    }

    #[test]
    fn wire_soup_captures_evenodd_multi_subpath() {
        let svg = r#"
            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
              <path fill-rule="evenodd" d="M0 0 L10 0 L10 10 L0 10 Z M3 3 L7 3 L7 7 L3 7 Z"/>
            </svg>
        "#;
        let soups = extract_svg_wire_soup(svg).expect("wire soup extracts");
        assert_eq!(soups.len(), 1, "one <path> => one soup");
        assert_eq!(soups[0].fill_rule, SvgFillRule::EvenOdd);
        assert_eq!(soups[0].wires.len(), 2, "two subpaths => two wires");
    }

    #[test]
    fn wire_soup_compound_icon_groups_per_path() {
        let svg = r#"
            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 30 10">
              <path d="M0 0 L10 0 L10 10 L0 10 Z"/>
              <path fill-rule="evenodd" d="M20 0 L30 0 L30 10 L20 10 Z"/>
            </svg>
        "#;
        let soups = extract_svg_wire_soup(svg).expect("wire soup extracts");
        assert_eq!(soups.len(), 2, "two <path> elements => two soups");
        assert_eq!(soups[0].fill_rule, SvgFillRule::NonZero);
        assert_eq!(soups[1].fill_rule, SvgFillRule::EvenOdd);
        assert!(soups.iter().all(|soup| soup.wires.len() == 1));
    }

    /// Exact-curve parity: curved contours carry cubic segments whose control
    /// points went through the same affine fit as the flattened points.
    #[test]
    fn parse_svg_profile_carries_exact_curve_segments_through_fit() {
        // Curve bounds are 8 wide (x 1..9) and 6 tall (y 2..8, the cubic
        // extrema), centered at (5,5). Contain fit into 20×20 => scale 2.5.
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
            <path d="M1 5 C1 1 9 1 9 5 C9 9 1 9 1 5 Z"/>
        </svg>"##;
        let profile =
            parse_svg_profile(svg, Some(20.0), Some(20.0), SvgFitMode::Contain, true)
                .expect("parse");

        assert!(profile.outer_geometry.has_curves());
        assert_eq!(profile.outer_geometry.segments.len(), 2);
        let scale = profile.fit.scale_x;
        assert!((scale - 2.5).abs() < 1.0e-6, "unexpected fit scale {scale}");
        // Source anchor (1,5) => ((1-5)·s, 0); control (1,1) => ((1-5)·s, (1-5)·s);
        // segment end (9,5) => ((9-5)·s, 0).
        assert!((profile.outer_geometry.start[0] - -4.0 * scale).abs() < 1.0e-9);
        assert!(profile.outer_geometry.start[1].abs() < 1.0e-9);
        let SvgPathSegment::Cubic { c1, to, .. } = &profile.outer_geometry.segments[0] else {
            panic!("expected cubic first segment");
        };
        assert!((c1[0] - -4.0 * scale).abs() < 1.0e-9 && (c1[1] - -4.0 * scale).abs() < 1.0e-9);
        assert!((to[0] - 4.0 * scale).abs() < 1.0e-9 && to[1].abs() < 1.0e-9);
    }

    /// Banana-lineart class: individually clean loops that CROSS each other
    /// must not pass the clean fast path (they build faces with intersecting
    /// rings); they belong to the wire-soup resolver.
    #[test]
    fn parse_svg_profile_rejects_mutually_intersecting_loops() {
        // Big square outer + two rectangles inside it that cross each other.
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 40 40">
            <path d="M0 0 H40 V40 H0 Z"/>
            <path d="M8 14 H30 V22 H8 Z"/>
            <path d="M14 8 H22 V30 H14 Z"/>
        </svg>"##;
        let error = parse_svg_profile(svg, Some(40.0), Some(40.0), SvgFitMode::Contain, true)
            .expect_err("mutually intersecting loops must fall back to wire soup");
        assert!(
            error.message.contains("intersect each other"),
            "unexpected error: {error:?}"
        );
    }

    /// TTF quads elevate to cubics losslessly: endpoints match and the cubic's
    /// midpoint equals the quadratic's midpoint (t=0.5 identity of degree
    /// elevation).
    #[test]
    fn quad_to_cubic_elevation_is_lossless_at_endpoints_and_midpoint() {
        let anchor = [0.0, 0.0];
        let control = Point::from_xy(2.0, 4.0);
        let end = Point::from_xy(4.0, 0.0);
        let SvgPathSegment::Cubic { c1, c2, to } =
            quad_to_cubic(anchor, control, end, Transform::identity())
        else {
            panic!("expected cubic");
        };
        assert_eq!(to, [4.0, 0.0]);
        // Quadratic at t=0.5: 0.25*q0 + 0.5*q1 + 0.25*q2 = (2, 2).
        let cubic_mid = [
            0.125 * anchor[0] + 0.375 * c1[0] + 0.375 * c2[0] + 0.125 * to[0],
            0.125 * anchor[1] + 0.375 * c1[1] + 0.375 * c2[1] + 0.125 * to[1],
        ];
        assert!((cubic_mid[0] - 2.0).abs() < 1.0e-12);
        assert!((cubic_mid[1] - 2.0).abs() < 1.0e-12);
    }
}
