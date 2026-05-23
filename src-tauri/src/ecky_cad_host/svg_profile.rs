use crate::contracts::{AppError, AppResult};
use std::str::FromStr;
use usvg::tiny_skia_path::{PathSegment, Point};
use usvg::{self, Node, Transform, Tree, Visibility};

const EPS: f64 = 1e-9;
const CURVE_SAMPLES: usize = 12;
const QUAD_SAMPLES: usize = 8;

#[derive(Debug, Clone, PartialEq)]
pub struct SvgProfile {
    pub outer_loop: Vec<[f64; 2]>,
    pub hole_loops: Vec<Vec<[f64; 2]>>,
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

#[derive(Debug, Clone, Default)]
struct ProfileParseState {
    loops: Vec<Vec<[f64; 2]>>,
    has_visible_path: bool,
    has_raster_or_text: bool,
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

    let (outer_loop, hole_loops) = classify_loops(normalized, reject_multi_outer_first_slice)?;
    let fit = compute_fit(
        &outer_loop,
        &hole_loops,
        target_width,
        target_height,
        fit_mode,
    )?;

    let source_view_box = tree.view_box();
    Ok(SvgProfile {
        outer_loop: transform_loop(outer_loop, &fit),
        hole_loops: hole_loops
            .into_iter()
            .map(|points| transform_loop(points, &fit))
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
    contours: &mut Vec<Vec<[f64; 2]>>,
) -> AppResult<()> {
    let mut current: Vec<[f64; 2]> = Vec::new();
    let mut start: Option<[f64; 2]> = None;

    let mut finalize = |current: &mut Vec<[f64; 2]>,
                        start: &mut Option<[f64; 2]>,
                        close_explicit: bool|
     -> AppResult<()> {
        if current.len() < 2 {
            current.clear();
            *start = None;
            return Ok(());
        }

        let loop_points = current.clone();
        *current = Vec::new();
        let mut cleaned = normalize_contour(loop_points)?;

        if cleaned.len() < 3 {
            return Err(AppError::validation(
                "SVG path contour has fewer than three distinct points.",
            ));
        }

        if !close_explicit && !points_equal(cleaned[0], cleaned[cleaned.len() - 1]) {
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
            return Err(AppError::validation(
                "SVG path contour has fewer than three points.",
            ));
        }

        if self_intersects(&cleaned) {
            return Err(AppError::validation(
                "SVG path contour is self-intersecting and cannot be used as profile geometry.",
            ));
        }

        if signed_area(&cleaned).abs() < EPS {
            return Err(AppError::validation("SVG path contour has near-zero area."));
        }

        let _ = start.take();
        contours.push(cleaned);
        Ok(())
    };

    for segment in path_data.segments() {
        match segment {
            PathSegment::MoveTo(point) => {
                if !current.is_empty() {
                    finalize(&mut current, &mut start, false)?;
                }
                let point = transform_point(point, abs_transform);
                current.push(point);
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
                }
            }
            PathSegment::QuadTo(c1, c2) => {
                if start.is_none() || current.is_empty() {
                    return Err(AppError::validation(
                        "SVG path contour missing a starting point.",
                    ));
                }
                let anchor = *current.last().expect("non-empty");
                sample_quad(anchor, c1, c2, abs_transform, &mut current);
            }
            PathSegment::CubicTo(c1, c2, c3) => {
                if start.is_none() || current.is_empty() {
                    return Err(AppError::validation(
                        "SVG path contour missing a starting point.",
                    ));
                }
                let anchor = *current.last().expect("non-empty");
                sample_cubic(anchor, c1, c2, c3, abs_transform, &mut current);
            }
            PathSegment::Close => {
                if start.is_none() {
                    continue;
                }
                finalize(&mut current, &mut start, true)?;
            }
        }
    }

    if !current.is_empty() {
        finalize(&mut current, &mut start, false)?;
    }

    Ok(())
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

fn normalize_loops(contours: Vec<Vec<[f64; 2]>>) -> AppResult<Vec<Vec<[f64; 2]>>> {
    let mut normalized = Vec::with_capacity(contours.len());
    for contour in contours {
        let mut points = normalize_contour(contour)?;
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

        if signed_area(&points) < 0.0 {
            points.reverse();
        }
        normalized.push(points);
    }
    Ok(normalized)
}

fn classify_loops(
    loops: Vec<Vec<[f64; 2]>>,
    reject_multi_outer_first_slice: bool,
) -> AppResult<(Vec<[f64; 2]>, Vec<Vec<[f64; 2]>>)> {
    let mut parent: Vec<Option<usize>> = vec![None; loops.len()];

    for i in 0..loops.len() {
        let rep = polygon_representative_point(&loops[i]);
        let mut parent_area = f64::INFINITY;
        let mut parent_index = None;

        for j in 0..loops.len() {
            if i == j {
                continue;
            }
            let candidate_area = signed_area(&loops[j]).abs();
            if candidate_area <= signed_area(&loops[i]).abs() {
                continue;
            }
            if point_in_polygon(rep, &loops[j]) && candidate_area < parent_area {
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
            signed_area(&loops[*left])
                .abs()
                .total_cmp(&signed_area(&loops[*right]).abs())
        })
        .ok_or_else(|| AppError::validation("SVG profile ingestion found no outer loops."))?;

    let mut holes = Vec::new();
    for (index, points) in loops.iter().enumerate() {
        if index == outer_index {
            continue;
        }
        if is_descendant(index, outer_index, &parent) {
            let mut hole = points.clone();
            if signed_area(&hole) > 0.0 {
                hole.reverse();
            }
            holes.push(hole);
        }
    }

    if holes.is_empty() {
        let mut outer_only = loops[outer_index].clone();
        if signed_area(&outer_only) < 0.0 {
            outer_only.reverse();
        }
        return Ok((outer_only, holes));
    }

    let mut outer = loops[outer_index].clone();
    if signed_area(&outer) < 0.0 {
        outer.reverse();
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
        .map(|point| {
            [
                (point[0] + fit.translate_x) * fit.scale_x,
                (point[1] + fit.translate_y) * fit.scale_y,
            ]
        })
        .collect()
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
}
