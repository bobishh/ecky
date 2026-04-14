use std::collections::BTreeMap;

use csgrs::float_types::parry3d::na::{self, Point3, Vector3};
use csgrs::mesh::polygon::Polygon as IrPolygon;
use csgrs::mesh::vertex::Vertex as IrVertex;
use csgrs::traits::CSG;
use geo::algorithm::contains::Contains;
use geo::algorithm::orient::{Direction, Orient};
use geo::{
    Coord, Geometry as GeoGeometry, GeometryCollection, LineString, MultiPolygon,
    Polygon as GeoPolygon,
};

use crate::ecky_ir_patterns::ContourSweepSlice;
use crate::models::{AppResult, ParamValue};

use super::eval_scalar::{approx_eq, eval_bool, eval_number, eval_points, parse_count};
use super::model::{expr_head_symbol, expr_keyword_name, expr_list_items, IrExpr};
use super::shared::{validation, IrMesh, IrSketch, LoopPoints};

#[derive(Clone, Debug)]
pub(super) struct ContourPolygon2d {
    outer: LoopPoints,
    holes: Vec<LoopPoints>,
}

#[derive(Clone, Debug)]
pub(super) struct ContourSet2d {
    polygons: Vec<ContourPolygon2d>,
}

#[derive(Clone, Debug)]
pub(super) struct SketchContours {
    outer_loops: Vec<LoopPoints>,
    hole_loops: Vec<LoopPoints>,
}

pub(super) fn cubic_bezier(
    p0: [f64; 3],
    p1: [f64; 3],
    p2: [f64; 3],
    p3: [f64; 3],
    t: f64,
) -> [f64; 3] {
    let mt = 1.0 - t;
    let c0 = mt * mt * mt;
    let c1 = 3.0 * mt * mt * t;
    let c2 = 3.0 * mt * t * t;
    let c3 = t * t * t;
    [
        c0 * p0[0] + c1 * p1[0] + c2 * p2[0] + c3 * p3[0],
        c0 * p0[1] + c1 * p1[1] + c2 * p2[1] + c3 * p3[1],
        c0 * p0[2] + c1 * p1[2] + c2 * p2[2] + c3 * p3[2],
    ]
}

pub(super) fn cubic_bezier_tangent(
    p0: [f64; 3],
    p1: [f64; 3],
    p2: [f64; 3],
    p3: [f64; 3],
    t: f64,
) -> [f64; 3] {
    let mt = 1.0 - t;
    let c0 = 3.0 * mt * mt;
    let c1 = 6.0 * mt * t;
    let c2 = 3.0 * t * t;
    [
        c0 * (p1[0] - p0[0]) + c1 * (p2[0] - p1[0]) + c2 * (p3[0] - p2[0]),
        c0 * (p1[1] - p0[1]) + c1 * (p2[1] - p1[1]) + c2 * (p3[1] - p2[1]),
        c0 * (p1[2] - p0[2]) + c1 * (p2[2] - p1[2]) + c2 * (p3[2] - p2[2]),
    ]
}

pub(super) fn sample_bezier_path(
    points: &[[f64; 3]],
    segments_per_segment: usize,
) -> AppResult<Vec<([f64; 3], [f64; 3])>> {
    if points.len() < 4 || !(points.len() - 1).is_multiple_of(3) {
        return Err(validation(
            "`bezier-path` expects 4, 7, 10, ... points (3n+1).",
        ));
    }
    let mut sampled = Vec::new();
    for i in (0..points.len() - 1).step_by(3) {
        let p0 = points[i];
        let p1 = points[i + 1];
        let p2 = points[i + 2];
        let p3 = points[i + 3];
        for step in 0..segments_per_segment {
            let t = step as f64 / segments_per_segment as f64;
            sampled.push((
                cubic_bezier(p0, p1, p2, p3, t),
                cubic_bezier_tangent(p0, p1, p2, p3, t),
            ));
        }
    }
    let last_p = *points.last().unwrap();
    let last_p0 = points[points.len() - 4];
    let last_p1 = points[points.len() - 3];
    let last_p2 = points[points.len() - 2];
    let last_p3 = points[points.len() - 1];
    sampled.push((
        last_p,
        cubic_bezier_tangent(last_p0, last_p1, last_p2, last_p3, 1.0),
    ));
    Ok(sampled)
}

pub(super) fn normalize_loop_points(points: &[[f64; 2]], context: &str) -> AppResult<LoopPoints> {
    let mut normalized = points.to_vec();
    if normalized.len() > 1 {
        let first = normalized[0];
        let last = *normalized.last().expect("checked above");
        if approx_eq(first[0], last[0]) && approx_eq(first[1], last[1]) {
            normalized.pop();
        }
    }
    if normalized.len() < 3 {
        return Err(validation(format!(
            "{} needs at least three unique points.",
            context
        )));
    }
    Ok(normalized)
}

pub(super) fn normalize_loop_from_coords(
    coords: &[Coord<f64>],
    context: &str,
) -> AppResult<LoopPoints> {
    let points: Vec<[f64; 2]> = coords.iter().map(|coord| [coord.x, coord.y]).collect();
    normalize_loop_points(&points, context)
}

pub(super) fn loop_centroid(points: &[[f64; 2]]) -> [f64; 2] {
    let mut x = 0.0;
    let mut y = 0.0;
    let len = points.len().max(1) as f64;
    for point in points {
        x += point[0];
        y += point[1];
    }
    [x / len, y / len]
}

pub(super) fn contour_sort_key(points: &[[f64; 2]]) -> (i64, i64, i64) {
    let centroid = loop_centroid(points);
    let area = signed_area(points).abs();
    (
        (centroid[0] * 1000.0).round() as i64,
        (centroid[1] * 1000.0).round() as i64,
        (area * 1000.0).round() as i64,
    )
}

pub(super) fn signed_area(points: &[[f64; 2]]) -> f64 {
    if points.len() < 3 {
        return 0.0;
    }
    let mut area = 0.0;
    for (index, point) in points.iter().enumerate() {
        let next = points[(index + 1) % points.len()];
        area += point[0] * next[1] - next[0] * point[1];
    }
    area * 0.5
}

pub(super) fn build_ring(points: &[[f64; 2]]) -> LineString<f64> {
    let mut coords: Vec<Coord<f64>> = points
        .iter()
        .map(|point| Coord {
            x: point[0],
            y: point[1],
        })
        .collect();
    if let Some(first) = coords.first().copied() {
        if coords.last().copied() != Some(first) {
            coords.push(first);
        }
    }
    LineString::new(coords)
}

pub(super) fn sketch_contours_from_loops(
    contours: SketchContours,
    context: &str,
) -> AppResult<ContourSet2d> {
    if contours.outer_loops.is_empty() {
        return Err(validation(format!(
            "{} needs at least one outer loop.",
            context
        )));
    }

    let outer_polygons: Vec<GeoPolygon<f64>> = contours
        .outer_loops
        .iter()
        .map(|loop_points| {
            GeoPolygon::new(build_ring(loop_points), vec![]).orient(Direction::Default)
        })
        .collect();

    let mut holes_by_outer: Vec<Vec<LoopPoints>> = vec![Vec::new(); outer_polygons.len()];
    for hole in &contours.hole_loops {
        let probe = Coord {
            x: loop_centroid(hole)[0],
            y: loop_centroid(hole)[1],
        };
        let outer_index = outer_polygons
            .iter()
            .enumerate()
            .find(|(_, polygon)| polygon.contains(&probe))
            .map(|(index, _)| index)
            .ok_or_else(|| {
                validation(format!(
                    "{} contains a hole loop that is not inside any outer loop.",
                    context
                ))
            })?;
        holes_by_outer[outer_index].push(hole.clone());
    }

    let mut polygons = outer_polygons
        .into_iter()
        .enumerate()
        .map(|(index, polygon)| ContourPolygon2d {
            outer: normalize_loop_from_coords(&polygon.exterior().0, context)
                .expect("normalized outer contour"),
            holes: holes_by_outer[index].clone(),
        })
        .collect::<Vec<_>>();

    sort_contour_set(&mut polygons);
    Ok(ContourSet2d { polygons })
}

pub(super) fn sort_contour_set(polygons: &mut [ContourPolygon2d]) {
    polygons.sort_by_key(|polygon| contour_sort_key(&polygon.outer));
    for polygon in polygons {
        polygon.holes.sort_by_key(|hole| contour_sort_key(hole));
    }
}

pub(super) fn contours_from_sketch(sketch: &IrSketch, context: &str) -> AppResult<ContourSet2d> {
    let multipolygon = sketch.to_multipolygon().orient(Direction::Default);
    let mut polygons = Vec::new();
    for polygon in multipolygon.0 {
        let outer = normalize_loop_from_coords(&polygon.exterior().0, context)?;
        let mut holes = Vec::new();
        for hole in polygon.interiors() {
            holes.push(normalize_loop_from_coords(&hole.0, context)?);
        }
        polygons.push(ContourPolygon2d { outer, holes });
    }

    if polygons.is_empty() {
        return Err(validation(format!(
            "{} needs at least one closed contour.",
            context
        )));
    }

    sort_contour_set(&mut polygons);
    Ok(ContourSet2d { polygons })
}

pub(super) fn contour_set_to_sketch(contours: &ContourSet2d) -> IrSketch {
    let multipolygon = MultiPolygon(
        contours
            .polygons
            .iter()
            .map(|polygon| {
                GeoPolygon::new(
                    build_ring(&polygon.outer),
                    polygon.holes.iter().map(|hole| build_ring(hole)).collect(),
                )
                .orient(Direction::Default)
            })
            .collect(),
    )
    .orient(Direction::Default);
    IrSketch::from_geo(
        GeometryCollection::from(vec![GeoGeometry::MultiPolygon(multipolygon)]),
        None,
    )
}

pub(super) fn eval_sketch_primitive_points(
    value: &IrExpr,
    env: &BTreeMap<String, ParamValue>,
    context: &str,
) -> AppResult<LoopPoints> {
    if let Some(items) = value.as_list() {
        if let Ok(node) = expr_head_symbol(items, context) {
            let args = &items[1..];
            match node {
                "circle" => {
                    let radius = eval_number(&args[0], env)?;
                    let segments = args
                        .get(1)
                        .map(|arg| eval_number(arg, env))
                        .transpose()?
                        .unwrap_or(48.0) as usize;
                    return normalize_loop_points(&circle_points(radius, segments.max(3)), context);
                }
                "polygon" => {
                    return normalize_loop_points(&eval_points(&args[0], env)?, context);
                }
                "rounded_rect" | "rounded-rect" => {
                    let width = eval_number(&args[0], env)?;
                    let height = eval_number(&args[1], env)?;
                    let corner_radius = eval_number(&args[2], env)?;
                    let segments = args
                        .get(3)
                        .map(|arg| eval_number(arg, env))
                        .transpose()?
                        .unwrap_or(12.0) as usize;
                    return normalize_loop_points(
                        &rounded_rectangle_points(width, height, corner_radius, segments.max(2)),
                        context,
                    );
                }
                "rounded-polygon" | "rounded_polygon" => {
                    let points = eval_points(&args[0], env)?;
                    let radius = eval_number(&args[1], env)?;
                    let segments = args
                        .get(2)
                        .map(|arg| parse_count(arg, env, "rounded-polygon segments", 2))
                        .transpose()?
                        .unwrap_or(8usize);
                    return normalize_loop_points(
                        &rounded_polygon_points(&points, radius, segments)?,
                        context,
                    );
                }
                "bspline" => {
                    let points = eval_points(&args[0], env)?;
                    let closed = args
                        .get(1)
                        .map(|arg| eval_bool(arg, env))
                        .transpose()?
                        .unwrap_or(true);
                    let samples = args
                        .get(2)
                        .map(|arg| parse_count(arg, env, "bspline samples", 1))
                        .transpose()?
                        .unwrap_or(12usize);
                    return normalize_loop_points(
                        &bspline_points(&points, samples, closed)?,
                        context,
                    );
                }
                _ => {}
            }
        }
    }

    normalize_loop_points(&eval_points(value, env)?, context)
}

pub(super) fn parse_loop_collection(
    value: &IrExpr,
    env: &BTreeMap<String, ParamValue>,
    context: &str,
) -> AppResult<Vec<LoopPoints>> {
    let items = expr_list_items(value, context)?;
    if items.is_empty() {
        return Ok(Vec::new());
    }

    // If it looks like a single node call (symbol followed by args) or a single point list.
    let is_node = items
        .first()
        .and_then(IrExpr::as_symbol)
        .map(|s| !s.starts_with(':'))
        .unwrap_or(false);

    if is_node {
        return Ok(vec![eval_sketch_primitive_points(value, env, context)?]);
    }

    let is_single_loop = items
        .first()
        .and_then(IrExpr::as_list)
        .map(|pair| pair.len() == 2)
        .unwrap_or(false);

    if is_single_loop {
        return Ok(vec![normalize_loop_points(
            &eval_points(value, env)?,
            context,
        )?]);
    }

    items
        .iter()
        .map(|loop_value| eval_sketch_primitive_points(loop_value, env, context))
        .collect()
}

pub(super) fn parse_profile_sketch(
    args: &[IrExpr],
    env: &BTreeMap<String, ParamValue>,
) -> AppResult<IrSketch> {
    if args.is_empty() {
        return Err(validation("`profile` expects contour data."));
    }

    if args.len() == 1 && expr_keyword_name(&args[0]).is_none() {
        return Ok(IrSketch::polygon(&eval_points(&args[0], env)?, None));
    }

    let mut outer_loops: Vec<LoopPoints> = Vec::new();
    let mut hole_loops: Vec<LoopPoints> = Vec::new();

    if args.first().and_then(expr_keyword_name).is_some() {
        if args.len() % 2 != 0 {
            return Err(validation(
                "`profile` clauses must use keyword/value pairs for `:outer` and `:holes`.",
            ));
        }
        let mut index = 0usize;
        while index < args.len() {
            let name = expr_keyword_name(&args[index]).ok_or_else(|| {
                validation("`profile` clauses must use keywords like `:outer` and `:holes`.")
            })?;
            let value = &args[index + 1];
            match name {
                "outer" => {
                    outer_loops.extend(parse_loop_collection(value, env, "profile outer loops")?)
                }
                "holes" => {
                    hole_loops.extend(parse_loop_collection(value, env, "profile hole loops")?)
                }
                other => {
                    return Err(validation(format!(
                        "`profile` does not recognize clause `:{}`.",
                        other
                    )))
                }
            }
            index += 2;
        }
    } else {
        for value in args {
            let pair = expr_list_items(value, "profile clause")?;
            if pair.len() != 2 {
                return Err(validation(
                    "`profile` clauses must look like `(:outer ...)` or `(:holes ...)`.",
                ));
            }
            let name = expr_keyword_name(&pair[0]).ok_or_else(|| {
                validation("`profile` clauses must use keywords like `:outer` and `:holes`.")
            })?;
            match name {
                "outer" => {
                    outer_loops.extend(parse_loop_collection(&pair[1], env, "profile outer loops")?)
                }
                "holes" => {
                    hole_loops.extend(parse_loop_collection(&pair[1], env, "profile hole loops")?)
                }
                other => {
                    return Err(validation(format!(
                        "`profile` does not recognize clause `:{}`.",
                        other
                    )))
                }
            }
        }
    }

    let contours = sketch_contours_from_loops(
        SketchContours {
            outer_loops,
            hole_loops,
        },
        "profile",
    )?;
    Ok(contour_set_to_sketch(&contours))
}

pub(super) fn distance2(point: [f64; 2], other: [f64; 2]) -> f64 {
    ((other[0] - point[0]).powi(2) + (other[1] - point[1]).powi(2)).sqrt()
}

pub(super) fn normalize2(point: [f64; 2]) -> [f64; 2] {
    let length = (point[0] * point[0] + point[1] * point[1]).sqrt();
    if length <= f64::EPSILON {
        [0.0, 0.0]
    } else {
        [point[0] / length, point[1] / length]
    }
}

pub(super) fn circle_points(radius: f64, segments: usize) -> Vec<[f64; 2]> {
    let mut points = Vec::with_capacity(segments);
    for i in 0..segments {
        let angle = 2.0 * std::f64::consts::PI * i as f64 / segments as f64;
        points.push([radius * angle.cos(), radius * angle.sin()]);
    }
    points
}

pub(super) fn rounded_rectangle_points(
    width: f64,
    height: f64,
    radius: f64,
    segments: usize,
) -> Vec<[f64; 2]> {
    let r = radius.min(width / 2.0).min(height / 2.0);
    let mut points = Vec::new();

    let centers = [
        [width / 2.0 - r, height / 2.0 - r],
        [-width / 2.0 + r, height / 2.0 - r],
        [-width / 2.0 + r, -height / 2.0 + r],
        [width / 2.0 - r, -height / 2.0 + r],
    ];

    let angles = [(0.0, 90.0), (90.0, 180.0), (180.0, 270.0), (270.0, 360.0)];

    for i in 0..4 {
        let center = centers[i];
        let (start, end) = angles[i];
        for j in 0..=segments {
            let a = (start + (end - start) * j as f64 / segments as f64).to_radians();
            points.push([center[0] + r * a.cos(), center[1] + r * a.sin()]);
        }
    }

    points
}

pub(super) fn rounded_polygon_points(
    points: &[[f64; 2]],
    radius: f64,
    segments: usize,
) -> AppResult<LoopPoints> {
    let base = normalize_loop_points(points, "`rounded-polygon`")?;
    if radius <= 0.0 {
        return Ok(base);
    }

    let segment_count = segments.max(2);
    let mut rounded = Vec::new();
    let polygon_area = signed_area(&base);
    let ccw = polygon_area >= 0.0;

    for index in 0..base.len() {
        let prev = base[(index + base.len() - 1) % base.len()];
        let current = base[index];
        let next = base[(index + 1) % base.len()];

        let incoming = normalize2([prev[0] - current[0], prev[1] - current[1]]);
        let outgoing = normalize2([next[0] - current[0], next[1] - current[1]]);
        let incoming_len = distance2(prev, current);
        let outgoing_len = distance2(current, next);
        if incoming_len <= f64::EPSILON || outgoing_len <= f64::EPSILON {
            continue;
        }

        let dot = ((-incoming[0] * outgoing[0]) + (-incoming[1] * outgoing[1])).clamp(-1.0, 1.0);
        let interior_angle = dot.acos();
        if interior_angle <= 1e-4 || (std::f64::consts::PI - interior_angle).abs() <= 1e-4 {
            rounded.push(current);
            continue;
        }

        let offset = (radius / (interior_angle * 0.5).tan())
            .min(incoming_len * 0.5)
            .min(outgoing_len * 0.5);
        if offset <= f64::EPSILON {
            rounded.push(current);
            continue;
        }

        let start = [
            current[0] + incoming[0] * offset,
            current[1] + incoming[1] * offset,
        ];
        let end = [
            current[0] + outgoing[0] * offset,
            current[1] + outgoing[1] * offset,
        ];
        let bisector = normalize2([(-incoming[0]) + outgoing[0], (-incoming[1]) + outgoing[1]]);
        let center_distance = radius / (interior_angle * 0.5).sin();
        let turn = (outgoing[0] - (-incoming[0])) * (outgoing[1] + incoming[1]);
        let center_sign = if ccw { 1.0 } else { -1.0 };
        let adjusted_bisector = if bisector == [0.0, 0.0] || !turn.is_finite() {
            [-incoming[1] * center_sign, incoming[0] * center_sign]
        } else {
            bisector
        };
        let center = [
            current[0] + adjusted_bisector[0] * center_distance,
            current[1] + adjusted_bisector[1] * center_distance,
        ];

        let start_angle = (start[1] - center[1]).atan2(start[0] - center[0]);
        let end_angle = (end[1] - center[1]).atan2(end[0] - center[0]);
        let mut delta = end_angle - start_angle;
        if ccw && delta <= 0.0 {
            delta += std::f64::consts::TAU;
        } else if !ccw && delta >= 0.0 {
            delta -= std::f64::consts::TAU;
        }

        rounded.push(start);
        for segment in 1..segment_count {
            let t = segment as f64 / segment_count as f64;
            let theta = start_angle + delta * t;
            rounded.push([
                center[0] + radius * theta.cos(),
                center[1] + radius * theta.sin(),
            ]);
        }
        rounded.push(end);
    }

    normalize_loop_points(&rounded, "`rounded-polygon`")
}

pub(super) fn bspline_points(
    control_points: &[[f64; 2]],
    samples: usize,
    closed: bool,
) -> AppResult<LoopPoints> {
    if control_points.len() < 3 {
        return Err(validation("`bspline` needs at least three control points."));
    }

    let n = control_points.len();
    let mut points = Vec::new();
    let sample_count = samples.max(2);

    if closed {
        let mut cp = control_points.to_vec();
        cp.push(control_points[0]);
        cp.push(control_points[1]);
        cp.push(control_points[2]);

        for i in 0..n {
            let p0 = cp[i];
            let p1 = cp[i + 1];
            let p2 = cp[i + 2];
            let p3 = cp[i + 3];

            for j in 0..sample_count {
                let t = j as f64 / sample_count as f64;
                points.push(sample_cubic_bspline(p0, p1, p2, p3, t));
            }
        }
    } else {
        if n < 4 {
            return Err(validation(
                "Open `bspline` needs at least four control points for cubic interpolation.",
            ));
        }
        for i in 0..n - 3 {
            let p0 = control_points[i];
            let p1 = control_points[i + 1];
            let p2 = control_points[i + 2];
            let p3 = control_points[i + 3];

            for j in 0..sample_count {
                let t = j as f64 / sample_count as f64;
                points.push(sample_cubic_bspline(p0, p1, p2, p3, t));
            }
        }
        points.push(sample_cubic_bspline(
            control_points[n - 4],
            control_points[n - 3],
            control_points[n - 2],
            control_points[n - 1],
            1.0,
        ));
    }

    normalize_loop_points(&points, "`bspline`")
}

pub(super) fn sample_cubic_bspline(
    p0: [f64; 2],
    p1: [f64; 2],
    p2: [f64; 2],
    p3: [f64; 2],
    t: f64,
) -> [f64; 2] {
    let t2 = t * t;
    let t3 = t2 * t;

    let f1 = (-t3 + 3.0 * t2 - 3.0 * t + 1.0) / 6.0;
    let f2 = (3.0 * t3 - 6.0 * t2 + 4.0) / 6.0;
    let f3 = (-3.0 * t3 + 3.0 * t2 + 3.0 * t + 1.0) / 6.0;
    let f4 = t3 / 6.0;

    [
        f1 * p0[0] + f2 * p1[0] + f3 * p2[0] + f4 * p3[0],
        f1 * p0[1] + f2 * p1[1] + f3 * p2[1] + f4 * p3[1],
    ]
}

pub(super) fn resample_loop(points: &[[f64; 2]], target_count: usize) -> LoopPoints {
    let n = points.len();
    if n == 0 || target_count == 0 {
        return Vec::new();
    }
    if n == target_count {
        return points.to_vec();
    }

    let mut perimeter = 0.0;
    let mut segment_lengths = Vec::with_capacity(n);
    for i in 0..n {
        let p1 = points[i];
        let p2 = points[(i + 1) % n];
        let d = ((p2[0] - p1[0]).powi(2) + (p2[1] - p1[1]).powi(2)).sqrt();
        segment_lengths.push(d);
        perimeter += d;
    }

    let mut resampled = Vec::with_capacity(target_count);
    for i in 0..target_count {
        let target_d = (i as f64 / target_count as f64) * perimeter;
        let mut current_d = 0.0;
        let mut found = false;
        for j in 0..n {
            if current_d + segment_lengths[j] >= target_d - 1e-7 {
                let t = if segment_lengths[j] > 1e-9 {
                    ((target_d - current_d) / segment_lengths[j]).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let p1 = points[j];
                let p2 = points[(j + 1) % n];
                resampled.push([p1[0] + (p2[0] - p1[0]) * t, p1[1] + (p2[1] - p1[1]) * t]);
                found = true;
                break;
            }
            current_d += segment_lengths[j];
        }
        if !found {
            resampled.push(*points.last().unwrap());
        }
    }
    resampled
}

pub(super) fn contour_outer_loops(contours: &ContourSet2d) -> Vec<LoopPoints> {
    contours
        .polygons
        .iter()
        .map(|polygon| polygon.outer.clone())
        .collect()
}

pub(super) fn contour_hole_loops(contours: &ContourSet2d) -> Vec<LoopPoints> {
    contours
        .polygons
        .iter()
        .flat_map(|polygon| polygon.holes.clone())
        .collect()
}

pub(super) fn contour_all_loops(contours: &ContourSet2d) -> Vec<LoopPoints> {
    let mut loops = contour_outer_loops(contours);
    loops.extend(contour_hole_loops(contours));
    loops
}

pub(super) fn contour_sweep_slice_from_contours(
    contours: &ContourSet2d,
    blocked_loops: Vec<LoopPoints>,
    z: f64,
) -> ContourSweepSlice {
    ContourSweepSlice {
        z,
        outer_loops: contour_outer_loops(contours),
        blocked_loops,
    }
}
pub(super) fn align_contour_sets(
    left: &ContourSet2d,
    right: &ContourSet2d,
    node: &str,
) -> AppResult<(ContourSet2d, ContourSet2d)> {
    if left.polygons.len() != right.polygons.len() {
        return Err(validation(format!(
            "Node `{}` needs contour sets with the same number of outer loops.",
            node
        )));
    }

    let mut aligned_left = Vec::with_capacity(left.polygons.len());
    let mut aligned_right = Vec::with_capacity(right.polygons.len());
    for (left_polygon, right_polygon) in left.polygons.iter().zip(&right.polygons) {
        if left_polygon.holes.len() != right_polygon.holes.len() {
            return Err(validation(format!(
                "Node `{}` needs matching hole topology between contours.",
                node
            )));
        }

        let outer_count = left_polygon.outer.len().max(right_polygon.outer.len());
        let mut left_holes = Vec::with_capacity(left_polygon.holes.len());
        let mut right_holes = Vec::with_capacity(right_polygon.holes.len());
        for (left_hole, right_hole) in left_polygon.holes.iter().zip(&right_polygon.holes) {
            let count = left_hole.len().max(right_hole.len());
            left_holes.push(resample_loop(left_hole, count));
            right_holes.push(resample_loop(right_hole, count));
        }

        aligned_left.push(ContourPolygon2d {
            outer: resample_loop(&left_polygon.outer, outer_count),
            holes: left_holes,
        });
        aligned_right.push(ContourPolygon2d {
            outer: resample_loop(&right_polygon.outer, outer_count),
            holes: right_holes,
        });
    }

    Ok((
        ContourSet2d {
            polygons: aligned_left,
        },
        ContourSet2d {
            polygons: aligned_right,
        },
    ))
}

pub(super) fn append_cap_polygons(
    polygons: &mut Vec<IrPolygon<()>>,
    contours: &ContourSet2d,
    z: f64,
    flip: bool,
) {
    for polygon in &contours.polygons {
        let holes = polygon
            .holes
            .iter()
            .map(|hole| hole.as_slice())
            .collect::<Vec<_>>();
        for triangle in IrSketch::triangulate_2d(&polygon.outer, &holes) {
            let verts = triangle
                .into_iter()
                .map(|point| IrVertex::new(Point3::new(point.x, point.y, z), Vector3::zeros()))
                .collect::<Vec<_>>();
            let mut poly = IrPolygon::new(verts, None);
            if flip {
                poly.flip();
            }
            polygons.push(poly);
        }
    }
}

pub(super) fn append_loop_side_polygons(
    polygons: &mut Vec<IrPolygon<()>>,
    bottom: &[[f64; 2]],
    bottom_z: f64,
    top: &[[f64; 2]],
    top_z: f64,
    flip: bool,
) {
    for index in 0..bottom.len() {
        let next = (index + 1) % bottom.len();
        let b0 = IrVertex::new(
            Point3::new(bottom[index][0], bottom[index][1], bottom_z),
            Vector3::zeros(),
        );
        let b1 = IrVertex::new(
            Point3::new(bottom[next][0], bottom[next][1], bottom_z),
            Vector3::zeros(),
        );
        let t1 = IrVertex::new(
            Point3::new(top[next][0], top[next][1], top_z),
            Vector3::zeros(),
        );
        let t0 = IrVertex::new(
            Point3::new(top[index][0], top[index][1], top_z),
            Vector3::zeros(),
        );
        let mut poly = IrPolygon::new(vec![b0, b1, t1, t0], None);
        if flip {
            poly.flip();
        }
        polygons.push(poly);
    }
}

pub(super) fn append_contour_side_polygons(
    polygons: &mut Vec<IrPolygon<()>>,
    bottom: &ContourSet2d,
    bottom_z: f64,
    top: &ContourSet2d,
    top_z: f64,
    flip: bool,
) {
    for (bottom_polygon, top_polygon) in bottom.polygons.iter().zip(&top.polygons) {
        append_loop_side_polygons(
            polygons,
            &bottom_polygon.outer,
            bottom_z,
            &top_polygon.outer,
            top_z,
            flip,
        );
        for (bottom_hole, top_hole) in bottom_polygon.holes.iter().zip(&top_polygon.holes) {
            append_loop_side_polygons(polygons, bottom_hole, bottom_z, top_hole, top_z, !flip);
        }
    }
}

pub(super) fn loft_between_contour_sets(
    bottom: &ContourSet2d,
    bottom_z: f64,
    top: &ContourSet2d,
    top_z: f64,
    node: &str,
) -> AppResult<IrMesh> {
    let (bottom_aligned, top_aligned) = align_contour_sets(bottom, top, node)?;
    let mut polygons = Vec::new();
    append_cap_polygons(&mut polygons, &bottom_aligned, bottom_z, true);
    append_cap_polygons(&mut polygons, &top_aligned, top_z, false);
    append_contour_side_polygons(
        &mut polygons,
        &bottom_aligned,
        bottom_z,
        &top_aligned,
        top_z,
        false,
    );
    if polygons.is_empty() {
        return Err(validation(format!("`{}` produced no geometry.", node)));
    }
    Ok(IrMesh::from_polygons(&polygons, None))
}

pub(super) fn loft_between_sketches(
    bottom: &IrSketch,
    bottom_z: f64,
    top: &IrSketch,
    top_z: f64,
    node: &str,
) -> AppResult<IrMesh> {
    let bottom_contours = contours_from_sketch(bottom, node)?;
    let top_contours = contours_from_sketch(top, node)?;
    loft_between_contour_sets(&bottom_contours, bottom_z, &top_contours, top_z, node)
}

pub(super) fn offset_sketch(
    sketch: &IrSketch,
    distance: f64,
    rounded: bool,
    node: &str,
) -> AppResult<IrSketch> {
    let shifted = if rounded {
        sketch.offset_rounded(distance)
    } else {
        sketch.offset(distance)
    };
    if shifted.to_multipolygon().0.is_empty() {
        return Err(validation(format!(
            "Node `{}` collapsed the sketch at offset distance {}.",
            node, distance
        )));
    }
    Ok(shifted)
}

pub(super) fn loft_segments(mut slices: Vec<(IrSketch, f64)>, node: &str) -> AppResult<IrMesh> {
    if slices.len() < 2 {
        return Err(validation(format!(
            "Node `{}` needs at least two slices.",
            node
        )));
    }
    let (first_sketch, first_z) = slices.remove(0);
    let mut current_contours = contours_from_sketch(&first_sketch, node)?;
    let mut current_z = first_z;
    let mut polygons = Vec::new();

    for (next_sketch, next_z) in slices {
        let next_contours = contours_from_sketch(&next_sketch, node)?;
        let (aligned_current, aligned_next) =
            align_contour_sets(&current_contours, &next_contours, node)?;
        if polygons.is_empty() {
            append_cap_polygons(&mut polygons, &aligned_current, current_z, true);
        }
        append_contour_side_polygons(
            &mut polygons,
            &aligned_current,
            current_z,
            &aligned_next,
            next_z,
            false,
        );
        current_contours = aligned_next;
        current_z = next_z;
    }
    append_cap_polygons(&mut polygons, &current_contours, current_z, false);

    if polygons.is_empty() {
        return Err(validation(format!("Node `{}` produced no geometry.", node)));
    }
    Ok(IrMesh::from_polygons(&polygons, None))
}

pub(super) fn contour_difference(
    outer: &ContourSet2d,
    inner: &ContourSet2d,
    node: &str,
) -> AppResult<ContourSet2d> {
    let ring = contour_set_to_sketch(outer).difference(&contour_set_to_sketch(inner));
    contours_from_sketch(&ring, node)
}

pub(super) fn append_shell_cap_polygons(
    polygons: &mut Vec<IrPolygon<()>>,
    outer: &ContourSet2d,
    inner: &ContourSet2d,
    z: f64,
    flip: bool,
    node: &str,
) -> AppResult<()> {
    let cap = contour_difference(outer, inner, node)?;
    append_cap_polygons(polygons, &cap, z, flip);
    Ok(())
}

pub(super) fn shell_from_contour_slices(
    mut outer_slices: Vec<(ContourSet2d, f64)>,
    mut inner_slices: Vec<(ContourSet2d, f64)>,
    node: &str,
) -> AppResult<IrMesh> {
    if outer_slices.len() < 2 || inner_slices.len() < 2 || outer_slices.len() != inner_slices.len()
    {
        return Err(validation(format!(
            "Node `{}` needs matching outer/inner slice stacks.",
            node
        )));
    }

    let (first_outer, first_z) = outer_slices.remove(0);
    let (first_inner, inner_first_z) = inner_slices.remove(0);
    if !approx_eq(first_z, inner_first_z) {
        return Err(validation(format!(
            "Node `{}` needs aligned outer/inner slice heights.",
            node
        )));
    }

    let mut current_outer = first_outer;
    let mut current_inner = first_inner;
    let mut current_z = first_z;
    let mut polygons = Vec::new();
    let mut start_outer = None;
    let mut start_inner = None;

    for ((next_outer, next_outer_z), (next_inner, next_inner_z)) in
        outer_slices.into_iter().zip(inner_slices.into_iter())
    {
        if !approx_eq(next_outer_z, next_inner_z) {
            return Err(validation(format!(
                "Node `{}` needs aligned outer/inner slice heights.",
                node
            )));
        }
        let (aligned_outer, aligned_next_outer) =
            align_contour_sets(&current_outer, &next_outer, node)?;
        let (aligned_inner, aligned_next_inner) =
            align_contour_sets(&current_inner, &next_inner, node)?;
        if start_outer.is_none() {
            start_outer = Some(aligned_outer.clone());
            start_inner = Some(aligned_inner.clone());
        }
        append_contour_side_polygons(
            &mut polygons,
            &aligned_outer,
            current_z,
            &aligned_next_outer,
            next_outer_z,
            false,
        );
        append_contour_side_polygons(
            &mut polygons,
            &aligned_inner,
            current_z,
            &aligned_next_inner,
            next_inner_z,
            true,
        );
        current_outer = aligned_next_outer;
        current_inner = aligned_next_inner;
        current_z = next_outer_z;
    }

    let start_outer = start_outer.unwrap_or_else(|| current_outer.clone());
    let start_inner = start_inner.unwrap_or_else(|| current_inner.clone());
    append_shell_cap_polygons(
        &mut polygons,
        &start_outer,
        &start_inner,
        first_z,
        true,
        node,
    )?;
    append_shell_cap_polygons(
        &mut polygons,
        &current_outer,
        &current_inner,
        current_z,
        false,
        node,
    )?;

    if polygons.is_empty() {
        return Err(validation(format!("Node `{}` produced no geometry.", node)));
    }
    Ok(IrMesh::from_polygons(&polygons, None))
}

pub(super) fn append_cap_polygons_transformed(
    polygons: &mut Vec<IrPolygon<()>>,
    contours: &ContourSet2d,
    transform: &na::Isometry3<f64>,
    flip: bool,
) {
    for polygon in &contours.polygons {
        let holes = polygon
            .holes
            .iter()
            .map(|hole| hole.as_slice())
            .collect::<Vec<_>>();
        for triangle in IrSketch::triangulate_2d(&polygon.outer, &holes) {
            let verts = triangle
                .into_iter()
                .map(|point| {
                    IrVertex::new(
                        transform * Point3::new(point.x, point.y, 0.0),
                        Vector3::zeros(),
                    )
                })
                .collect::<Vec<_>>();
            let mut poly = IrPolygon::new(verts, None);
            if flip {
                poly.flip();
            }
            polygons.push(poly);
        }
    }
}

pub(super) fn append_loop_side_polygons_transformed(
    polygons: &mut Vec<IrPolygon<()>>,
    bottom: &[[f64; 2]],
    bottom_transform: &na::Isometry3<f64>,
    top: &[[f64; 2]],
    top_transform: &na::Isometry3<f64>,
    flip: bool,
) {
    for index in 0..bottom.len() {
        let next = (index + 1) % bottom.len();
        let b0 = IrVertex::new(
            bottom_transform * Point3::new(bottom[index][0], bottom[index][1], 0.0),
            Vector3::zeros(),
        );
        let b1 = IrVertex::new(
            bottom_transform * Point3::new(bottom[next][0], bottom[next][1], 0.0),
            Vector3::zeros(),
        );
        let t1 = IrVertex::new(
            top_transform * Point3::new(top[next][0], top[next][1], 0.0),
            Vector3::zeros(),
        );
        let t0 = IrVertex::new(
            top_transform * Point3::new(top[index][0], top[index][1], 0.0),
            Vector3::zeros(),
        );
        let mut poly = IrPolygon::new(vec![b0, b1, t1, t0], None);
        if flip {
            poly.flip();
        }
        polygons.push(poly);
    }
}

pub(super) fn append_contour_side_polygons_transformed(
    polygons: &mut Vec<IrPolygon<()>>,
    bottom: &ContourSet2d,
    bottom_transform: &na::Isometry3<f64>,
    top: &ContourSet2d,
    top_transform: &na::Isometry3<f64>,
    flip: bool,
) {
    for (bottom_polygon, top_polygon) in bottom.polygons.iter().zip(&top.polygons) {
        append_loop_side_polygons_transformed(
            polygons,
            &bottom_polygon.outer,
            bottom_transform,
            &top_polygon.outer,
            top_transform,
            flip,
        );
        for (bottom_hole, top_hole) in bottom_polygon.holes.iter().zip(&top_polygon.holes) {
            append_loop_side_polygons_transformed(
                polygons,
                bottom_hole,
                bottom_transform,
                top_hole,
                top_transform,
                !flip,
            );
        }
    }
}

pub(super) fn loft_segments_transformed(
    mut slices: Vec<(ContourSet2d, na::Isometry3<f64>)>,
    node: &str,
) -> AppResult<IrMesh> {
    if slices.len() < 2 {
        return Err(validation(format!(
            "Node `{}` needs at least two slices.",
            node
        )));
    }
    let (first_contours, first_transform) = slices.remove(0);
    let mut current_contours = first_contours;
    let mut current_transform = first_transform;
    let mut polygons = Vec::new();

    for (next_contours, next_transform) in slices {
        let (aligned_current, aligned_next) =
            align_contour_sets(&current_contours, &next_contours, node)?;
        if polygons.is_empty() {
            append_cap_polygons_transformed(
                &mut polygons,
                &aligned_current,
                &current_transform,
                true,
            );
        }
        append_contour_side_polygons_transformed(
            &mut polygons,
            &aligned_current,
            &current_transform,
            &aligned_next,
            &next_transform,
            false,
        );
        current_contours = aligned_next;
        current_transform = next_transform;
    }
    append_cap_polygons_transformed(&mut polygons, &current_contours, &current_transform, false);

    if polygons.is_empty() {
        return Err(validation(format!("Node `{}` produced no geometry.", node)));
    }
    Ok(IrMesh::from_polygons(&polygons, None))
}
