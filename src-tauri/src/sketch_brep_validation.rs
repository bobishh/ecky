use crate::models::{
    BrepHiddenLineProjectionResponse, BrepProjectedEdge2d, BrepProjectedLoop2d,
    BrepProjectedLoopRole, SketchBrepProjectionValidation, SketchDefinition, SketchDocument,
    SketchPrimitive, SketchPrimitiveKind, SketchValidationIssue, SketchValidationSeverity,
    SketchView,
};

#[derive(Debug, Clone, Copy, PartialEq)]
struct Bounds2d {
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct BoundsDelta {
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
}

#[derive(Debug, Clone)]
struct ClosedProfile<'a> {
    sketch: &'a SketchDefinition,
    primitive: &'a SketchPrimitive,
    bounds: Bounds2d,
}

#[derive(Debug, Clone, PartialEq)]
struct ContainmentMismatch {
    edge_id: String,
    outside_count: usize,
    max_outside: f64,
}

#[derive(Debug, Clone, PartialEq)]
struct TopologyMismatch {
    loop_count: usize,
    hole_count: usize,
    source_profile_count: usize,
    source_hole_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
struct TopologyCounts {
    loop_count: usize,
    hole_count: usize,
    source_profile_count: usize,
    source_hole_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Point2Key {
    x: i64,
    y: i64,
}

#[derive(Debug, Clone)]
struct LoopSegment {
    edge_id: String,
    a: [f64; 2],
    b: [f64; 2],
    a_key: Point2Key,
    b_key: Point2Key,
}

impl Point2Key {
    fn new(point: [f64; 2]) -> Self {
        Self {
            x: (point[0] * 1_000_000.0).round() as i64,
            y: (point[1] * 1_000_000.0).round() as i64,
        }
    }
}

pub fn validate_sketch_brep_hidden_line_projection(
    document: &SketchDocument,
    projection: &BrepHiddenLineProjectionResponse,
    tolerance: f64,
) -> SketchBrepProjectionValidation {
    let tolerance = if tolerance.is_finite() && tolerance >= 0.0 {
        tolerance
    } else {
        0.0
    };
    let mut issues = Vec::new();
    let mut evidence = Vec::new();

    for view in [SketchView::Front, SketchView::Top, SketchView::Side] {
        let label = view_label(&view);
        let Some(profile) = find_closed_profile(document, &view) else {
            issues.push(SketchValidationIssue {
                sketch_id: view_key(&view).to_string(),
                primitive_id: None,
                severity: SketchValidationSeverity::Error,
                message: format!("{label} sketch view has no active closed profile."),
            });
            continue;
        };

        let Some(brep_bounds) = projected_bounds(projection, &view) else {
            issues.push(SketchValidationIssue {
                sketch_id: profile.sketch.sketch_id.clone(),
                primitive_id: Some(profile.primitive.primitive_id.clone()),
                severity: SketchValidationSeverity::Error,
                message: format!("{label} BRep projection has no visible or hidden edge points."),
            });
            continue;
        };

        let delta = profile.bounds.delta(brep_bounds);
        let max_delta = delta.max_component();
        if max_delta > tolerance {
            issues.push(SketchValidationIssue {
                sketch_id: profile.sketch.sketch_id.clone(),
                primitive_id: Some(profile.primitive.primitive_id.clone()),
                severity: SketchValidationSeverity::Error,
                message: format!(
                    "{label} bounds mismatch: sketch {}, brep {}, maxDelta={:.6}, tolerance={:.6}.",
                    profile.bounds.format_values(),
                    brep_bounds.format_values(),
                    max_delta,
                    tolerance
                ),
            });
            continue;
        } else {
            evidence.push(format!(
                "{label} bounds match within tolerance {:.6}: sketch {}, brep {}, maxDelta={:.6}.",
                tolerance,
                profile.bounds.format_values(),
                brep_bounds.format_values(),
                max_delta
            ));
        }

        if let Some(mismatch) = containment_mismatch(&profile, projection, &view, tolerance) {
            issues.push(SketchValidationIssue {
                sketch_id: profile.sketch.sketch_id.clone(),
                primitive_id: Some(profile.primitive.primitive_id.clone()),
                severity: SketchValidationSeverity::Error,
                message: format!(
                    "{label} containment mismatch: edge {} has {} samples outside source profile, maxOutside={:.6}, tolerance={:.6}.",
                    mismatch.edge_id, mismatch.outside_count, mismatch.max_outside, tolerance
                ),
            });
        } else {
            evidence.push(format!(
                "{label} containment pass within tolerance {:.6}: {} projection samples inside source profile.",
                tolerance,
                projected_sample_count(projection, &view)
            ));
        }

        if let Some(mismatch) = topology_mismatch(&profile, projection, &view) {
            issues.push(SketchValidationIssue {
                sketch_id: profile.sketch.sketch_id.clone(),
                primitive_id: Some(profile.primitive.primitive_id.clone()),
                severity: SketchValidationSeverity::Error,
                message: format!(
                    "{label} topology mismatch: BRep projection has {} closed loops ({} holes) but source sketch has {}.",
                    mismatch.loop_count,
                    mismatch.hole_count,
                    source_topology_description(&mismatch),
                ),
            });
        } else if let Some(counts) = topology_counts(&profile, projection, &view) {
            evidence.push(format!(
                "{label} topology pass: {} BRep loops, {} source {}, {} holes.",
                counts.loop_count,
                counts.source_profile_count,
                profile_count_label(counts.source_profile_count),
                counts.hole_count
            ));
        }
    }

    SketchBrepProjectionValidation {
        passed: issues.is_empty(),
        issues,
        evidence,
    }
}

fn find_closed_profile<'a>(
    document: &'a SketchDocument,
    view: &SketchView,
) -> Option<ClosedProfile<'a>> {
    let active_sketch_id = document.active_sketch_id.as_deref();
    let active_profile = active_sketch_id.and_then(|active_id| {
        document
            .sketches
            .iter()
            .filter(|sketch| sketch.sketch_id == active_id && sketch.view == *view)
            .find_map(closed_profile_for_sketch)
    });

    active_profile.or_else(|| {
        document
            .sketches
            .iter()
            .filter(|sketch| sketch.view == *view)
            .find_map(closed_profile_for_sketch)
    })
}

fn closed_profile_for_sketch(sketch: &SketchDefinition) -> Option<ClosedProfile<'_>> {
    sketch.primitives.iter().find_map(|primitive| {
        if !primitive.closed {
            return None;
        }
        primitive_bounds(primitive).map(|bounds| ClosedProfile {
            sketch,
            primitive,
            bounds,
        })
    })
}

fn primitive_bounds(primitive: &SketchPrimitive) -> Option<Bounds2d> {
    match primitive.kind {
        SketchPrimitiveKind::Circle => {
            let radius = primitive.radius?;
            if !radius.is_finite() || radius <= 0.0 {
                return None;
            }
            let center = primitive.points.first().copied().unwrap_or([0.0, 0.0]);
            if !center[0].is_finite() || !center[1].is_finite() {
                return None;
            }
            Some(Bounds2d {
                min_x: center[0] - radius,
                max_x: center[0] + radius,
                min_y: center[1] - radius,
                max_y: center[1] + radius,
            })
        }
        _ => Bounds2d::from_points(&primitive.points),
    }
}

fn projected_bounds(
    projection: &BrepHiddenLineProjectionResponse,
    view: &SketchView,
) -> Option<Bounds2d> {
    projected_edges(projection, view)
        .into_iter()
        .filter_map(edge_bounds)
        .reduce(Bounds2d::union)
}

fn projected_edges<'a>(
    projection: &'a BrepHiddenLineProjectionResponse,
    view: &SketchView,
) -> Vec<&'a BrepProjectedEdge2d> {
    projection
        .views
        .iter()
        .filter(|candidate| candidate.view == *view)
        .flat_map(|candidate| {
            candidate
                .visible_edges
                .iter()
                .chain(candidate.hidden_edges.iter())
        })
        .collect()
}

fn projected_sample_count(
    projection: &BrepHiddenLineProjectionResponse,
    view: &SketchView,
) -> usize {
    projected_edges(projection, view)
        .into_iter()
        .map(|edge| edge.points.len())
        .sum()
}

fn edge_bounds(edge: &BrepProjectedEdge2d) -> Option<Bounds2d> {
    Bounds2d::from_points(&edge.points)
}

fn containment_mismatch(
    profile: &ClosedProfile<'_>,
    projection: &BrepHiddenLineProjectionResponse,
    view: &SketchView,
    tolerance: f64,
) -> Option<ContainmentMismatch> {
    projected_edges(projection, view)
        .into_iter()
        .filter_map(|edge| edge_containment_mismatch(profile, edge, tolerance))
        .max_by(|a, b| {
            a.max_outside
                .partial_cmp(&b.max_outside)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

fn edge_containment_mismatch(
    profile: &ClosedProfile<'_>,
    edge: &BrepProjectedEdge2d,
    tolerance: f64,
) -> Option<ContainmentMismatch> {
    let mut outside_count = 0usize;
    let mut max_outside = 0.0f64;
    for point in &edge.points {
        if profile_contains_point(profile.primitive, *point, tolerance) {
            continue;
        }
        outside_count += 1;
        max_outside = max_outside.max(profile_outside_distance(profile.primitive, *point));
    }
    (outside_count > 0).then(|| ContainmentMismatch {
        edge_id: edge.edge_id.clone(),
        outside_count,
        max_outside,
    })
}

fn topology_mismatch(
    profile: &ClosedProfile<'_>,
    projection: &BrepHiddenLineProjectionResponse,
    view: &SketchView,
) -> Option<TopologyMismatch> {
    let counts = topology_counts(profile, projection, view)?;
    (counts.loop_count != counts.source_profile_count
        || counts.hole_count != counts.source_hole_count)
        .then_some(TopologyMismatch {
            loop_count: counts.loop_count,
            hole_count: counts.hole_count,
            source_profile_count: counts.source_profile_count,
            source_hole_count: counts.source_hole_count,
        })
}

fn topology_counts(
    profile: &ClosedProfile<'_>,
    projection: &BrepHiddenLineProjectionResponse,
    view: &SketchView,
) -> Option<TopologyCounts> {
    let loops = projected_loops_for_view(projection, view);
    if loops.is_empty() {
        return None;
    }
    let hole_count = loops
        .iter()
        .filter(|item| item.role == BrepProjectedLoopRole::Hole)
        .count();
    let source_profiles = source_profile_roles(profile.sketch);
    let source_hole_count = source_profiles
        .iter()
        .filter(|role| **role == BrepProjectedLoopRole::Hole)
        .count();
    Some(TopologyCounts {
        loop_count: loops.len(),
        hole_count,
        source_profile_count: source_profiles.len(),
        source_hole_count,
    })
}

fn source_profile_roles(sketch: &SketchDefinition) -> Vec<BrepProjectedLoopRole> {
    let profiles = sketch
        .primitives
        .iter()
        .filter(|primitive| primitive.closed)
        .filter_map(|primitive| {
            Some(SourceProfileTopology {
                primitive,
                area: primitive_area_abs(primitive)?,
                sample: primitive_representative_point(primitive)?,
            })
        })
        .collect::<Vec<_>>();

    profiles
        .iter()
        .enumerate()
        .map(|(index, profile)| {
            let containing_larger_count = profiles
                .iter()
                .enumerate()
                .filter(|(other_index, other)| {
                    *other_index != index
                        && other.area > profile.area
                        && profile_contains_point(other.primitive, profile.sample, 0.0)
                })
                .count();
            if containing_larger_count % 2 == 1 {
                BrepProjectedLoopRole::Hole
            } else {
                BrepProjectedLoopRole::Outer
            }
        })
        .collect()
}

#[derive(Debug, Clone, Copy)]
struct SourceProfileTopology<'a> {
    primitive: &'a SketchPrimitive,
    area: f64,
    sample: [f64; 2],
}

fn primitive_area_abs(primitive: &SketchPrimitive) -> Option<f64> {
    match primitive.kind {
        SketchPrimitiveKind::Circle => {
            let radius = primitive.radius?;
            if radius.is_finite() && radius > 0.0 {
                Some(std::f64::consts::PI * radius * radius)
            } else {
                None
            }
        }
        _ => {
            let area = polygon_area_abs(&primitive.points);
            (area > 0.0).then_some(area)
        }
    }
}

fn primitive_representative_point(primitive: &SketchPrimitive) -> Option<[f64; 2]> {
    match primitive.kind {
        SketchPrimitiveKind::Circle => primitive.points.first().copied().filter(finite_point),
        _ => primitive.points.iter().copied().find(finite_point),
    }
}

fn finite_point(point: &[f64; 2]) -> bool {
    point[0].is_finite() && point[1].is_finite()
}

fn profile_count_label(count: usize) -> &'static str {
    if count == 1 {
        "profile"
    } else {
        "profiles"
    }
}

fn source_topology_description(mismatch: &TopologyMismatch) -> String {
    if mismatch.loop_count == mismatch.source_profile_count {
        format!(
            "{} closed {} ({} holes)",
            mismatch.source_profile_count,
            profile_count_label(mismatch.source_profile_count),
            mismatch.source_hole_count
        )
    } else {
        format!(
            "{} closed {}",
            mismatch.source_profile_count,
            profile_count_label(mismatch.source_profile_count)
        )
    }
}

fn projected_loops_for_view(
    projection: &BrepHiddenLineProjectionResponse,
    view: &SketchView,
) -> Vec<BrepProjectedLoop2d> {
    for candidate in projection
        .views
        .iter()
        .filter(|candidate| candidate.view == *view)
    {
        if !candidate.loops.is_empty() {
            return classify_projected_loops(candidate.loops.clone());
        }
    }
    let edges = projected_edges(projection, view)
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    projected_loops_from_edges(&edges)
}

fn projected_loops_from_edges(edges: &[BrepProjectedEdge2d]) -> Vec<BrepProjectedLoop2d> {
    let mut unused = edges
        .iter()
        .flat_map(loop_segments_for_edge)
        .collect::<Vec<_>>();
    let mut loops = Vec::new();

    while let Some(first) = unused.pop() {
        let start_key = first.a_key;
        let mut current_key = first.b_key;
        let mut edge_ids = vec![first.edge_id.clone()];
        let mut points = vec![first.a, first.b];

        while current_key != start_key {
            let Some(next) = take_connected_segment(&mut unused, current_key) else {
                break;
            };
            current_key = next.1;
            edge_ids.push(next.2);
            points.push(next.0);
        }

        if current_key == start_key && edge_ids.len() >= 3 && points.len() >= 4 {
            loops.push(BrepProjectedLoop2d {
                loop_id: format!("loop{}", loops.len()),
                edge_ids,
                points,
                role: BrepProjectedLoopRole::Unknown,
                source_class: "derived".to_string(),
            });
        }
    }

    classify_projected_loops(loops)
}

fn loop_segments_for_edge(edge: &BrepProjectedEdge2d) -> Vec<LoopSegment> {
    edge.points
        .windows(2)
        .filter_map(|segment| {
            let a = segment[0];
            let b = segment[1];
            let a_key = Point2Key::new(a);
            let b_key = Point2Key::new(b);
            (a_key != b_key).then(|| LoopSegment {
                edge_id: edge.edge_id.clone(),
                a,
                b,
                a_key,
                b_key,
            })
        })
        .collect()
}

fn take_connected_segment(
    unused: &mut Vec<LoopSegment>,
    current_key: Point2Key,
) -> Option<([f64; 2], Point2Key, String)> {
    let index = unused
        .iter()
        .position(|segment| segment.a_key == current_key || segment.b_key == current_key)?;
    let segment = unused.swap_remove(index);
    if segment.a_key == current_key {
        Some((segment.b, segment.b_key, segment.edge_id))
    } else {
        Some((segment.a, segment.a_key, segment.edge_id))
    }
}

fn classify_projected_loops(mut loops: Vec<BrepProjectedLoop2d>) -> Vec<BrepProjectedLoop2d> {
    let areas = loops
        .iter()
        .map(|item| polygon_area_abs(&item.points))
        .collect::<Vec<_>>();
    for index in 0..loops.len() {
        if loops[index].role != BrepProjectedLoopRole::Unknown {
            continue;
        }
        let Some(sample) = representative_loop_point(&loops[index]) else {
            loops[index].role = BrepProjectedLoopRole::Unknown;
            continue;
        };
        let containing_larger_count = loops
            .iter()
            .enumerate()
            .filter(|(other_index, other)| {
                *other_index != index
                    && areas[*other_index] > areas[index]
                    && polygon_contains_point(&other.points, sample, 0.0)
            })
            .count();
        loops[index].role = if containing_larger_count % 2 == 1 {
            BrepProjectedLoopRole::Hole
        } else {
            BrepProjectedLoopRole::Outer
        };
    }
    loops.sort_by(|left, right| {
        loop_role_rank(&left.role)
            .cmp(&loop_role_rank(&right.role))
            .then_with(|| left.loop_id.cmp(&right.loop_id))
    });
    loops
}

fn loop_role_rank(role: &BrepProjectedLoopRole) -> u8 {
    match role {
        BrepProjectedLoopRole::Outer => 0,
        BrepProjectedLoopRole::Hole => 1,
        BrepProjectedLoopRole::Unknown => 2,
    }
}

fn representative_loop_point(loop2d: &BrepProjectedLoop2d) -> Option<[f64; 2]> {
    loop2d.points.iter().copied().find(|point| {
        point[0].is_finite()
            && point[1].is_finite()
            && Point2Key::new(*point) != Point2Key::new(*loop2d.points.last().unwrap_or(point))
    })
}

fn polygon_area_abs(points: &[[f64; 2]]) -> f64 {
    if points.len() < 3 {
        return 0.0;
    }
    let mut area = 0.0;
    for index in 0..points.len() {
        let next = (index + 1) % points.len();
        area += points[index][0] * points[next][1] - points[next][0] * points[index][1];
    }
    (area * 0.5).abs()
}

fn profile_contains_point(primitive: &SketchPrimitive, point: [f64; 2], tolerance: f64) -> bool {
    if !point[0].is_finite() || !point[1].is_finite() {
        return false;
    }
    match primitive.kind {
        SketchPrimitiveKind::Circle => {
            let Some(radius) = primitive.radius else {
                return false;
            };
            let center = primitive.points.first().copied().unwrap_or([0.0, 0.0]);
            if !radius.is_finite()
                || radius <= 0.0
                || !center[0].is_finite()
                || !center[1].is_finite()
            {
                return false;
            }
            point_distance(point, center) <= radius + tolerance
        }
        _ => polygon_contains_point(&primitive.points, point, tolerance),
    }
}

fn profile_outside_distance(primitive: &SketchPrimitive, point: [f64; 2]) -> f64 {
    match primitive.kind {
        SketchPrimitiveKind::Circle => {
            let radius = primitive.radius.unwrap_or(0.0);
            let center = primitive.points.first().copied().unwrap_or([0.0, 0.0]);
            (point_distance(point, center) - radius).max(0.0)
        }
        _ => polygon_boundary_distance(&primitive.points, point),
    }
}

fn polygon_contains_point(points: &[[f64; 2]], point: [f64; 2], tolerance: f64) -> bool {
    if points.len() < 3
        || points
            .iter()
            .any(|point| !point[0].is_finite() || !point[1].is_finite())
    {
        return false;
    }
    if polygon_boundary_distance(points, point) <= tolerance {
        return true;
    }

    let mut inside = false;
    let mut previous = points[points.len() - 1];
    for current in points {
        let yi = current[1];
        let yj = previous[1];
        let crosses_y = (yi > point[1]) != (yj > point[1]);
        if crosses_y {
            let x_at_y = (previous[0] - current[0]) * (point[1] - yi) / (yj - yi) + current[0];
            if point[0] < x_at_y {
                inside = !inside;
            }
        }
        previous = *current;
    }
    inside
}

fn polygon_boundary_distance(points: &[[f64; 2]], point: [f64; 2]) -> f64 {
    if points.len() < 2 {
        return f64::INFINITY;
    }
    let mut min_distance = f64::INFINITY;
    for segment in points.windows(2) {
        min_distance = min_distance.min(point_segment_distance(point, segment[0], segment[1]));
    }
    let first = points[0];
    let last = points[points.len() - 1];
    if first != last {
        min_distance = min_distance.min(point_segment_distance(point, last, first));
    }
    min_distance
}

fn point_segment_distance(point: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let ap = [point[0] - a[0], point[1] - a[1]];
    let ab_len_sq = ab[0] * ab[0] + ab[1] * ab[1];
    if ab_len_sq <= f64::EPSILON {
        return point_distance(point, a);
    }
    let t = ((ap[0] * ab[0] + ap[1] * ab[1]) / ab_len_sq).clamp(0.0, 1.0);
    point_distance(point, [a[0] + t * ab[0], a[1] + t * ab[1]])
}

fn point_distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

fn view_label(view: &SketchView) -> &'static str {
    match view {
        SketchView::Front => "Front",
        SketchView::Top => "Top",
        SketchView::Side => "Side",
        SketchView::Custom => "Custom",
    }
}

fn view_key(view: &SketchView) -> &'static str {
    match view {
        SketchView::Front => "front",
        SketchView::Top => "top",
        SketchView::Side => "side",
        SketchView::Custom => "custom",
    }
}

impl Bounds2d {
    fn from_points(points: &[[f64; 2]]) -> Option<Self> {
        let first = points.first()?;
        if !first[0].is_finite() || !first[1].is_finite() {
            return None;
        }
        let mut bounds = Self {
            min_x: first[0],
            max_x: first[0],
            min_y: first[1],
            max_y: first[1],
        };
        for point in points.iter().skip(1) {
            if !point[0].is_finite() || !point[1].is_finite() {
                return None;
            }
            bounds.min_x = bounds.min_x.min(point[0]);
            bounds.max_x = bounds.max_x.max(point[0]);
            bounds.min_y = bounds.min_y.min(point[1]);
            bounds.max_y = bounds.max_y.max(point[1]);
        }
        Some(bounds)
    }

    fn union(self, other: Self) -> Self {
        Self {
            min_x: self.min_x.min(other.min_x),
            max_x: self.max_x.max(other.max_x),
            min_y: self.min_y.min(other.min_y),
            max_y: self.max_y.max(other.max_y),
        }
    }

    fn delta(self, other: Self) -> BoundsDelta {
        BoundsDelta {
            min_x: (self.min_x - other.min_x).abs(),
            max_x: (self.max_x - other.max_x).abs(),
            min_y: (self.min_y - other.min_y).abs(),
            max_y: (self.max_y - other.max_y).abs(),
        }
    }

    fn format_values(self) -> String {
        format!(
            "minX={:.6} maxX={:.6} minY={:.6} maxY={:.6}",
            self.min_x, self.max_x, self.min_y, self.max_y
        )
    }
}

impl BoundsDelta {
    fn max_component(self) -> f64 {
        self.min_x.max(self.max_x).max(self.min_y).max(self.max_y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn polyline(id: &str, points: Vec<[f64; 2]>) -> SketchPrimitive {
        SketchPrimitive {
            primitive_id: id.to_string(),
            kind: SketchPrimitiveKind::Polyline,
            points,
            closed: true,
            radius: None,
        }
    }

    fn sketch(sketch_id: &str, view: SketchView, primitive: SketchPrimitive) -> SketchDefinition {
        SketchDefinition {
            sketch_id: sketch_id.to_string(),
            view,
            plane: None,
            primitives: vec![primitive],
            constraints: Vec::new(),
        }
    }

    fn document_with_front(front: SketchPrimitive) -> SketchDocument {
        SketchDocument {
            document_id: "doc".to_string(),
            sketches: vec![
                sketch("front-sketch", SketchView::Front, front),
                sketch(
                    "top-sketch",
                    SketchView::Top,
                    polyline(
                        "top-profile",
                        vec![
                            [0.0, 0.0],
                            [10.0, 0.0],
                            [10.0, 10.0],
                            [0.0, 10.0],
                            [0.0, 0.0],
                        ],
                    ),
                ),
                sketch(
                    "side-sketch",
                    SketchView::Side,
                    polyline(
                        "side-profile",
                        vec![
                            [0.0, 0.0],
                            [10.0, 0.0],
                            [10.0, 10.0],
                            [0.0, 10.0],
                            [0.0, 0.0],
                        ],
                    ),
                ),
            ],
            active_sketch_id: None,
            units: Some("mm".to_string()),
            metadata: None,
        }
    }

    fn edge(edge_id: &str, points: Vec<[f64; 2]>) -> BrepProjectedEdge2d {
        BrepProjectedEdge2d {
            edge_id: edge_id.to_string(),
            points,
            source_class: "V".to_string(),
        }
    }

    fn view(
        view: SketchView,
        edges: Vec<BrepProjectedEdge2d>,
    ) -> crate::models::BrepHiddenLineProjectionView {
        crate::models::BrepHiddenLineProjectionView {
            view,
            direction: [0.0, 0.0, 1.0],
            visible_edges: edges,
            hidden_edges: Vec::new(),
            loops: Vec::new(),
        }
    }

    fn base_rect_view(
        view_name: SketchView,
        prefix: &str,
    ) -> crate::models::BrepHiddenLineProjectionView {
        view(
            view_name,
            vec![
                edge(&format!("{prefix}-bottom"), vec![[0.0, 0.0], [10.0, 0.0]]),
                edge(&format!("{prefix}-right"), vec![[10.0, 0.0], [10.0, 10.0]]),
                edge(&format!("{prefix}-top"), vec![[10.0, 10.0], [0.0, 10.0]]),
                edge(&format!("{prefix}-left"), vec![[0.0, 10.0], [0.0, 0.0]]),
            ],
        )
    }

    fn projection(front_edges: Vec<BrepProjectedEdge2d>) -> BrepHiddenLineProjectionResponse {
        BrepHiddenLineProjectionResponse {
            model_id: "model".to_string(),
            source_artifact_path: "/tmp/model.step".to_string(),
            views: vec![
                view(SketchView::Front, front_edges),
                base_rect_view(SketchView::Top, "top"),
                base_rect_view(SketchView::Side, "side"),
            ],
            warnings: Vec::new(),
            validation: None,
        }
    }

    fn l_profile() -> SketchPrimitive {
        polyline(
            "front-profile",
            vec![
                [0.0, 0.0],
                [100.0, 0.0],
                [100.0, 40.0],
                [40.0, 40.0],
                [40.0, 100.0],
                [0.0, 100.0],
                [0.0, 0.0],
            ],
        )
    }

    #[test]
    fn matching_projection_points_inside_closed_profile_pass() {
        let document = document_with_front(l_profile());
        let validation = validate_sketch_brep_hidden_line_projection(
            &document,
            &projection(vec![
                edge("front-bottom", vec![[0.0, 0.0], [100.0, 0.0]]),
                edge("front-right-leg", vec![[100.0, 0.0], [100.0, 40.0]]),
                edge("front-notch-bottom", vec![[100.0, 40.0], [40.0, 40.0]]),
                edge("front-notch-left", vec![[40.0, 40.0], [40.0, 100.0]]),
                edge("front-top", vec![[40.0, 100.0], [0.0, 100.0]]),
                edge("front-left", vec![[0.0, 100.0], [0.0, 0.0]]),
            ]),
            0.1,
        );

        assert!(validation.passed, "{validation:?}");
        assert!(validation
            .evidence
            .iter()
            .any(|item| item.contains("Front containment pass")));
    }

    #[test]
    fn same_bounds_projection_points_outside_concave_profile_fail() {
        let document = document_with_front(l_profile());
        let validation = validate_sketch_brep_hidden_line_projection(
            &document,
            &projection(vec![
                edge("front-bottom", vec![[0.0, 0.0], [100.0, 0.0]]),
                edge("front-right", vec![[100.0, 0.0], [100.0, 100.0]]),
                edge("front-top", vec![[100.0, 100.0], [0.0, 100.0]]),
                edge("front-left", vec![[0.0, 100.0], [0.0, 0.0]]),
            ]),
            0.1,
        );

        assert!(!validation.passed);
        let issue = validation
            .issues
            .iter()
            .find(|item| item.message.contains("Front containment mismatch"))
            .expect("front containment issue");
        assert_eq!(issue.sketch_id, "front-sketch");
        assert_eq!(issue.primitive_id.as_deref(), Some("front-profile"));
        assert!(issue.message.contains("front-right") || issue.message.contains("front-top"));
    }

    #[test]
    fn points_within_tolerance_of_profile_boundary_pass() {
        let document = document_with_front(polyline(
            "front-profile",
            vec![
                [0.0, 0.0],
                [10.0, 0.0],
                [10.0, 10.0],
                [0.0, 10.0],
                [0.0, 0.0],
            ],
        ));
        let validation = validate_sketch_brep_hidden_line_projection(
            &document,
            &projection(vec![
                edge("front-bottom", vec![[0.0, 0.0], [10.0, 0.0]]),
                edge("front-right", vec![[10.05, 0.0], [10.05, 10.0]]),
                edge("front-top", vec![[10.0, 10.0], [0.0, 10.0]]),
                edge("front-left", vec![[0.0, 10.0], [0.0, 0.0]]),
            ]),
            0.1,
        );

        assert!(validation.passed, "{validation:?}");
    }

    #[test]
    fn projected_loops_from_edges_reconstructs_unordered_outer_and_hole() {
        let loops = projected_loops_from_edges(&[
            edge("hole-top", vec![[7.0, 3.0], [3.0, 3.0]]),
            edge("outer-right", vec![[10.0, 0.0], [10.0, 5.0]]),
            edge("hole-left", vec![[3.0, 3.0], [3.0, 2.0]]),
            edge("outer-bottom", vec![[0.0, 0.0], [10.0, 0.0]]),
            edge("outer-left", vec![[0.0, 5.0], [0.0, 0.0]]),
            edge("hole-right", vec![[7.0, 2.0], [7.0, 3.0]]),
            edge("outer-top", vec![[10.0, 5.0], [0.0, 5.0]]),
            edge("hole-bottom", vec![[3.0, 2.0], [7.0, 2.0]]),
        ]);

        assert_eq!(loops.len(), 2);
        assert_eq!(
            loops
                .iter()
                .filter(|item| item.role == crate::models::BrepProjectedLoopRole::Outer)
                .count(),
            1
        );
        let hole = loops
            .iter()
            .find(|item| item.role == crate::models::BrepProjectedLoopRole::Hole)
            .expect("hole loop");
        assert_eq!(hole.edge_ids.len(), 4);
        assert!(hole.edge_ids.iter().any(|edge_id| edge_id == "hole-bottom"));
    }

    #[test]
    fn projected_loop_roles_use_containment_parity_for_nested_islands() {
        let loops = classify_projected_loops(vec![
            crate::models::BrepProjectedLoop2d {
                loop_id: "outer".to_string(),
                edge_ids: Vec::new(),
                points: vec![
                    [0.0, 0.0],
                    [10.0, 0.0],
                    [10.0, 10.0],
                    [0.0, 10.0],
                    [0.0, 0.0],
                ],
                role: crate::models::BrepProjectedLoopRole::Unknown,
                source_class: "derived".to_string(),
            },
            crate::models::BrepProjectedLoop2d {
                loop_id: "hole".to_string(),
                edge_ids: Vec::new(),
                points: vec![[2.0, 2.0], [8.0, 2.0], [8.0, 8.0], [2.0, 8.0], [2.0, 2.0]],
                role: crate::models::BrepProjectedLoopRole::Unknown,
                source_class: "derived".to_string(),
            },
            crate::models::BrepProjectedLoop2d {
                loop_id: "island".to_string(),
                edge_ids: Vec::new(),
                points: vec![[4.0, 4.0], [6.0, 4.0], [6.0, 6.0], [4.0, 6.0], [4.0, 4.0]],
                role: crate::models::BrepProjectedLoopRole::Unknown,
                source_class: "derived".to_string(),
            },
        ]);

        let outer_count = loops
            .iter()
            .filter(|item| item.role == crate::models::BrepProjectedLoopRole::Outer)
            .count();
        let hole_count = loops
            .iter()
            .filter(|item| item.role == crate::models::BrepProjectedLoopRole::Hole)
            .count();

        assert_eq!(outer_count, 2);
        assert_eq!(hole_count, 1);
        assert_eq!(
            loops
                .iter()
                .find(|item| item.loop_id == "island")
                .expect("island loop")
                .role,
            crate::models::BrepProjectedLoopRole::Outer
        );
    }
}
