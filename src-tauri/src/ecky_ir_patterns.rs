use std::num::NonZeroU32;
use std::sync::OnceLock;

use csgrs::float_types::parry3d::na::{Point3, Vector3};
use csgrs::mesh::plane::Plane;
use csgrs::mesh::Mesh;
use csgrs::traits::CSG;

use crate::models::{AppError, AppResult};

const MAX_SAFE_PATTERN_STL_BYTES: u64 = 64 * 1024 * 1024;

type IrMesh = Mesh<()>;
type LoopPoints = Vec<[f64; 2]>;

#[derive(Clone, Debug)]
pub enum WallPatternMode {
    Ribs,
    Rings,
    Spiral,
    Diamond,
    Hammered,
    Fourier,
}

#[derive(Clone, Debug)]
pub struct WallPatternSpec {
    pub mode: WallPatternMode,
    pub depth: f64,
    pub u_freq: f64,
    pub v_freq: f64,
    pub phase: f64,
    pub bias: f64,
    pub duty: f64,
    pub softness: f64,
    pub twist_deg: f64,
    pub seed: u64,
    pub rim_fade: f64,
}

#[derive(Clone, Debug)]
pub struct ContourSweepSlice {
    pub z: f64,
    pub outer_loops: Vec<LoopPoints>,
    pub blocked_loops: Vec<LoopPoints>,
}

#[derive(Clone, Debug)]
pub enum WallPatternTarget {
    ContourSweep {
        slices: Vec<ContourSweepSlice>,
    },
    RevolveProfile {
        angle_deg: f64,
        z_min: f64,
        z_max: f64,
        outer_loops: Vec<LoopPoints>,
        blocked_loops: Vec<LoopPoints>,
    },
    ShellCylinder {
        outer_radius: f64,
        inner_radius: f64,
        height: f64,
    },
    ShellCone {
        outer_bottom_radius: f64,
        outer_top_radius: f64,
        inner_bottom_radius: f64,
        inner_top_radius: f64,
        height: f64,
    },
    ShellSphere {
        outer_radius: f64,
        inner_radius: f64,
    },
}

fn validation(message: impl Into<String>) -> AppError {
    AppError::validation(message.into())
}

pub fn apply_wall_pattern(
    mesh: &IrMesh,
    target: &WallPatternTarget,
    spec: &WallPatternSpec,
) -> AppResult<IrMesh> {
    validate_pattern_spec(spec)?;
    let mut patterned = mesh.triangulate();
    let levels = recommended_subdivision_levels(&patterned, spec);
    ensure_pattern_budget(patterned.polygons.len(), levels)?;
    if let Some(levels) = NonZeroU32::new(levels) {
        patterned = patterned.subdivide_triangles(levels);
    }

    for polygon in &mut patterned.polygons {
        for vertex in &mut polygon.vertices {
            let amount = sample_displacement(vertex.pos, vertex.normal, target, spec)?;
            if amount.abs() <= f64::EPSILON {
                continue;
            }
            let direction = if vertex.normal.norm() > f64::EPSILON {
                vertex.normal.normalize()
            } else {
                vertex.normal
            };
            vertex.pos = Point3::new(
                vertex.pos.x + direction.x * amount,
                vertex.pos.y + direction.y * amount,
                vertex.pos.z + direction.z * amount,
            );
        }
        polygon.plane = Plane::from_vertices(polygon.vertices.clone());
        polygon.bounding_box = OnceLock::new();
    }
    patterned.bounding_box = OnceLock::new();
    Ok(patterned)
}

fn validate_pattern_spec(spec: &WallPatternSpec) -> AppResult<()> {
    if !spec.depth.is_finite() || spec.depth <= 0.0 {
        return Err(validation("`wall-pattern` requires `:depth` > 0."));
    }
    if !spec.u_freq.is_finite()
        || !spec.v_freq.is_finite()
        || spec.u_freq < 0.0
        || spec.v_freq < 0.0
    {
        return Err(validation(
            "`wall-pattern` frequencies must be finite numbers >= 0.",
        ));
    }
    if spec.u_freq <= 0.0 && spec.v_freq <= 0.0 {
        return Err(validation(
            "`wall-pattern` needs `:uFreq` or `:vFreq` above zero.",
        ));
    }
    if !spec.duty.is_finite() || spec.duty <= 0.0 || spec.duty >= 1.0 {
        return Err(validation(
            "`wall-pattern` `:duty` must be between 0 and 1.",
        ));
    }
    if !spec.softness.is_finite() || spec.softness < 0.0 || spec.softness >= 0.5 {
        return Err(validation(
            "`wall-pattern` `:softness` must be between 0 and 0.5.",
        ));
    }
    if !spec.rim_fade.is_finite() || spec.rim_fade < 0.0 || spec.rim_fade >= 0.5 {
        return Err(validation(
            "`wall-pattern` `:rimFade` must be between 0 and 0.5.",
        ));
    }
    Ok(())
}

fn recommended_subdivision_levels(mesh: &IrMesh, spec: &WallPatternSpec) -> u32 {
    let bb = mesh.bounding_box();
    let span_x = (bb.maxs.x - bb.mins.x).abs();
    let span_y = (bb.maxs.y - bb.mins.y).abs();
    let span_z = (bb.maxs.z - bb.mins.z).abs();
    let longest = span_x.max(span_y).max(span_z);
    let detail_signal = spec.u_freq.max(spec.v_freq.max(1.0)) * (longest / 32.0).max(1.0);
    if detail_signal >= 18.0 {
        2
    } else if detail_signal >= 8.0 {
        1
    } else {
        0
    }
}

fn ensure_pattern_budget(base_triangles: usize, levels: u32) -> AppResult<()> {
    let multiplier = 4usize.saturating_pow(levels);
    let estimated_triangles = base_triangles.saturating_mul(multiplier);
    let estimated_bytes = 84u64.saturating_add(50u64.saturating_mul(estimated_triangles as u64));
    if estimated_bytes > MAX_SAFE_PATTERN_STL_BYTES {
        return Err(validation(format!(
            "wall-pattern preview is too large for the viewer (estimated {} MB > {} MB). Reduce frequency, target size, or switch the thread engine to FreeCAD.",
            estimated_bytes / (1024 * 1024),
            MAX_SAFE_PATTERN_STL_BYTES / (1024 * 1024)
        )));
    }
    Ok(())
}

fn sample_displacement(
    pos: Point3<f64>,
    normal: Vector3<f64>,
    target: &WallPatternTarget,
    spec: &WallPatternSpec,
) -> AppResult<f64> {
    match target {
        WallPatternTarget::ContourSweep { slices } => {
            let first = slices.first().ok_or_else(|| {
                validation("wall-pattern contour sweep needs at least one slice.")
            })?;
            let last = slices.last().ok_or_else(|| {
                validation("wall-pattern contour sweep needs at least one slice.")
            })?;
            let t = normalize_range(pos.z, first.z, last.z);
            if should_skip_cap(normal, t) {
                return Ok(0.0);
            }
            let sweep = interpolate_contour_sweep(slices, pos.z)?;
            let point = [pos.x, pos.y];
            let sample = nearest_loops_sample(point, &sweep.outer_loops)
                .ok_or_else(|| validation("wall-pattern contour sweep has no outer loops."))?;
            if let Some(blocked) = nearest_loops_sample(point, &sweep.blocked_loops) {
                if blocked.distance <= sample.distance {
                    return Ok(0.0);
                }
            }
            Ok(pattern_amplitude(spec, sample.progress, t, false) * spec.depth)
        }
        WallPatternTarget::RevolveProfile {
            angle_deg,
            z_min,
            z_max,
            outer_loops,
            blocked_loops,
        } => {
            if radial_normal_strength(normal) < 0.2 {
                return Ok(0.0);
            }
            let radius = (pos.x * pos.x + pos.y * pos.y).sqrt();
            let profile_point = [radius, pos.z];
            let outer = nearest_loops_sample(profile_point, outer_loops)
                .ok_or_else(|| validation("wall-pattern revolve target has no outer profiles."))?;
            if let Some(blocked) = nearest_loops_sample(profile_point, blocked_loops) {
                if blocked.distance <= outer.distance {
                    return Ok(0.0);
                }
            }
            let u = angle_progress(pos.x, pos.y, *angle_deg);
            let v = normalize_range(pos.z, *z_min, *z_max);
            let _ = outer.progress;
            Ok(pattern_amplitude(spec, u, v, false) * spec.depth)
        }
        WallPatternTarget::ShellCylinder {
            outer_radius,
            inner_radius,
            height,
        } => {
            let radial = (pos.x * pos.x + pos.y * pos.y).sqrt();
            if !matches_outer_radius(radial, *outer_radius, *inner_radius) {
                return Ok(0.0);
            }
            if radial_normal_strength(normal) < 0.25 {
                return Ok(0.0);
            }
            let u = angle_progress(pos.x, pos.y, 360.0);
            let v = normalized_height(pos.z, *height);
            Ok(pattern_amplitude(spec, u, v, false) * spec.depth)
        }
        WallPatternTarget::ShellCone {
            outer_bottom_radius,
            outer_top_radius,
            inner_bottom_radius,
            inner_top_radius,
            height,
        } => {
            let v = normalized_height(pos.z, *height);
            let radial = (pos.x * pos.x + pos.y * pos.y).sqrt();
            let outer_radius = lerp(*outer_bottom_radius, *outer_top_radius, v);
            let inner_radius = lerp(*inner_bottom_radius, *inner_top_radius, v);
            if !matches_outer_radius(radial, outer_radius, inner_radius) {
                return Ok(0.0);
            }
            if radial_normal_strength(normal) < 0.2 {
                return Ok(0.0);
            }
            let u = angle_progress(pos.x, pos.y, 360.0);
            Ok(pattern_amplitude(spec, u, v, false) * spec.depth)
        }
        WallPatternTarget::ShellSphere {
            outer_radius,
            inner_radius,
        } => {
            let radius = (pos.x * pos.x + pos.y * pos.y + pos.z * pos.z).sqrt();
            if !matches_outer_radius(radius, *outer_radius, *inner_radius) {
                return Ok(0.0);
            }
            let outward = Vector3::new(pos.x, pos.y, pos.z);
            if outward.norm() <= f64::EPSILON || normal.dot(&outward.normalize()) <= 0.0 {
                return Ok(0.0);
            }
            let u = angle_progress(pos.x, pos.y, 360.0);
            let v = ((pos.z / outer_radius.max(1e-6)) + 1.0) * 0.5;
            Ok(pattern_amplitude(spec, u, v, true) * spec.depth)
        }
    }
}

fn interpolate_contour_sweep(slices: &[ContourSweepSlice], z: f64) -> AppResult<ContourSweepSlice> {
    if slices.is_empty() {
        return Err(validation(
            "wall-pattern contour sweep needs at least one slice.",
        ));
    }
    if slices.len() == 1 {
        return Ok(slices[0].clone());
    }

    for pair in slices.windows(2) {
        let start = &pair[0];
        let end = &pair[1];
        let z_min = start.z.min(end.z);
        let z_max = start.z.max(end.z);
        if z < z_min || z > z_max {
            continue;
        }
        let t = normalize_range(z, start.z, end.z);
        return Ok(ContourSweepSlice {
            z,
            outer_loops: interpolate_loop_sets(&start.outer_loops, &end.outer_loops, t)?,
            blocked_loops: interpolate_loop_sets(&start.blocked_loops, &end.blocked_loops, t)?,
        });
    }

    let closest = slices
        .iter()
        .min_by(|left, right| {
            (left.z - z)
                .abs()
                .partial_cmp(&(right.z - z).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .expect("checked above");
    Ok(closest.clone())
}

fn interpolate_loop_sets(
    start: &[LoopPoints],
    end: &[LoopPoints],
    t: f64,
) -> AppResult<Vec<LoopPoints>> {
    if start.len() != end.len() {
        return Err(validation(
            "wall-pattern contour sweep needs consistent loop topology across slices.",
        ));
    }
    start
        .iter()
        .zip(end.iter())
        .map(|(start_loop, end_loop)| {
            if start_loop.len() != end_loop.len() {
                return Err(validation(
                    "wall-pattern contour sweep needs matching vertex counts across slices.",
                ));
            }
            Ok(start_loop
                .iter()
                .zip(end_loop.iter())
                .map(|(start_point, end_point)| {
                    [
                        lerp(start_point[0], end_point[0], t),
                        lerp(start_point[1], end_point[1], t),
                    ]
                })
                .collect())
        })
        .collect()
}

fn should_skip_cap(normal: Vector3<f64>, v: f64) -> bool {
    (v <= 0.001 || v >= 0.999) && normal.z.abs() > 0.45
}

fn normalized_height(z: f64, height: f64) -> f64 {
    if height.abs() <= f64::EPSILON {
        0.5
    } else {
        (z / height).clamp(0.0, 1.0)
    }
}

fn normalize_range(value: f64, min: f64, max: f64) -> f64 {
    let span = (max - min).abs();
    if span <= f64::EPSILON {
        0.5
    } else {
        ((value - min) / (max - min)).clamp(0.0, 1.0)
    }
}

fn angle_progress(x: f64, y: f64, angle_deg: f64) -> f64 {
    let mut angle = y.atan2(x).to_degrees();
    if angle < 0.0 {
        angle += 360.0;
    }
    let span = angle_deg.abs().max(1.0);
    (angle / span).clamp(0.0, 1.0)
}

fn radial_normal_strength(normal: Vector3<f64>) -> f64 {
    (normal.x * normal.x + normal.y * normal.y).sqrt()
}

fn matches_outer_radius(actual: f64, outer: f64, inner: f64) -> bool {
    (actual - outer).abs() <= (actual - inner).abs()
}

fn lerp(start: f64, end: f64, t: f64) -> f64 {
    start + (end - start) * t
}

struct OutlineSample {
    progress: f64,
    distance: f64,
}

fn nearest_loops_sample(point: [f64; 2], loops: &[LoopPoints]) -> Option<OutlineSample> {
    loops
        .iter()
        .filter(|outline| !outline.is_empty())
        .map(|outline| nearest_outline_sample(point, outline))
        .min_by(|left, right| {
            left.distance
                .partial_cmp(&right.distance)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

fn nearest_outline_sample(point: [f64; 2], outline: &[[f64; 2]]) -> OutlineSample {
    let total_length = outline_perimeter(outline).max(1e-6);
    let mut cumulative = 0.0;
    let mut best_distance = f64::MAX;
    let mut best_progress = 0.0;

    for (index, start) in outline.iter().enumerate() {
        let end = outline[(index + 1) % outline.len()];
        let dx = end[0] - start[0];
        let dy = end[1] - start[1];
        let segment_len = (dx * dx + dy * dy).sqrt();
        if segment_len <= f64::EPSILON {
            continue;
        }
        let t = (((point[0] - start[0]) * dx) + ((point[1] - start[1]) * dy))
            / (segment_len * segment_len);
        let t = t.clamp(0.0, 1.0);
        let proj = [start[0] + dx * t, start[1] + dy * t];
        let dist = ((point[0] - proj[0]).powi(2) + (point[1] - proj[1]).powi(2)).sqrt();
        if dist < best_distance {
            best_distance = dist;
            best_progress = (cumulative + segment_len * t) / total_length;
        }
        cumulative += segment_len;
    }

    OutlineSample {
        progress: best_progress.clamp(0.0, 1.0),
        distance: best_distance,
    }
}

fn outline_perimeter(outline: &[[f64; 2]]) -> f64 {
    outline
        .iter()
        .enumerate()
        .map(|(index, start)| {
            let end = outline[(index + 1) % outline.len()];
            ((end[0] - start[0]).powi(2) + (end[1] - start[1]).powi(2)).sqrt()
        })
        .sum()
}

fn pattern_amplitude(spec: &WallPatternSpec, u: f64, v: f64, spherical: bool) -> f64 {
    let shear = spec.twist_deg / 360.0;
    let u_field = wrap01(u + v * shear + spec.phase);
    let v_field = wrap01(v + spec.phase);
    let mut raw = match spec.mode {
        WallPatternMode::Ribs => pulse(u_field * spec.u_freq.max(1.0), spec.duty, spec.softness),
        WallPatternMode::Rings => pulse(v_field * spec.v_freq.max(1.0), spec.duty, spec.softness),
        WallPatternMode::Spiral => pulse(
            (u + v * shear) * spec.u_freq.max(1.0) + v * spec.v_freq.max(1.0).max(1.0),
            spec.duty,
            spec.softness,
        ),
        WallPatternMode::Diamond => {
            let a = pulse(
                wrap01((u + v + shear * v) * spec.u_freq.max(1.0)),
                spec.duty,
                spec.softness,
            );
            let b = pulse(
                wrap01((u - v + shear * v) * spec.v_freq.max(spec.u_freq).max(1.0)),
                spec.duty,
                spec.softness,
            );
            ((a + b) * 0.5).clamp(0.0, 1.0)
        }
        WallPatternMode::Hammered => {
            let freq_u = spec.u_freq.max(1.0);
            let freq_v = spec.v_freq.max(spec.u_freq).max(1.0);
            let noise = value_noise_2d(u * freq_u, v * freq_v, spec.seed);
            smoothstep(spec.bias.clamp(-0.95, 0.95) * 0.5 + 0.25, 1.0, noise)
        }
        WallPatternMode::Fourier => {
            let u_wave = (u_field * spec.u_freq * 2.0 * std::f64::consts::PI).sin();
            let v_wave = (v_field * spec.v_freq * 2.0 * std::f64::consts::PI).sin();
            let combined = (u_wave + v_wave) * 0.5;
            (combined * 0.5 + 0.5).clamp(0.0, 1.0)
        }
    };

    raw = (raw + spec.bias).clamp(0.0, 1.0);
    let fade = rim_fade(v, spec.rim_fade);
    let polar_fade = if spherical {
        (std::f64::consts::PI * v).sin().abs().powf(0.75)
    } else {
        1.0
    };
    raw * fade * polar_fade
}

fn wrap01(value: f64) -> f64 {
    let mut wrapped = value.fract();
    if wrapped < 0.0 {
        wrapped += 1.0;
    }
    wrapped
}

fn pulse(value: f64, duty: f64, softness: f64) -> f64 {
    let x = wrap01(value);
    let left = smoothstep(0.0, softness.max(1e-4), x);
    let right = 1.0 - smoothstep((duty - softness).max(0.0), (duty + softness).min(1.0), x);
    (left * right).clamp(0.0, 1.0)
}

fn rim_fade(v: f64, rim_fade: f64) -> f64 {
    if rim_fade <= 0.0 {
        return 1.0;
    }
    let edge = rim_fade.min(0.49);
    let start = smoothstep(0.0, edge, v);
    let end = smoothstep(0.0, edge, 1.0 - v);
    (start * end).clamp(0.0, 1.0)
}

fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    if (edge1 - edge0).abs() <= f64::EPSILON {
        return if x >= edge1 { 1.0 } else { 0.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn hash_u64(mut value: u64) -> u64 {
    value ^= value >> 33;
    value = value.wrapping_mul(0xff51afd7ed558ccd);
    value ^= value >> 33;
    value = value.wrapping_mul(0xc4ceb9fe1a85ec53);
    value ^ (value >> 33)
}

fn random_from_grid(x: i64, y: i64, seed: u64) -> f64 {
    let mixed = hash_u64((x as u64).wrapping_mul(0x9e3779b97f4a7c15) ^ (y as u64) ^ seed);
    (mixed as f64 / u64::MAX as f64).clamp(0.0, 1.0)
}

fn value_noise_2d(x: f64, y: f64, seed: u64) -> f64 {
    let x0 = x.floor() as i64;
    let y0 = y.floor() as i64;
    let xf = x - x.floor();
    let yf = y - y.floor();
    let n00 = random_from_grid(x0, y0, seed);
    let n10 = random_from_grid(x0 + 1, y0, seed);
    let n01 = random_from_grid(x0, y0 + 1, seed);
    let n11 = random_from_grid(x0 + 1, y0 + 1, seed);
    let sx = smoothstep(0.0, 1.0, xf);
    let sy = smoothstep(0.0, 1.0, yf);
    let ix0 = lerp(n00, n10, sx);
    let ix1 = lerp(n01, n11, sx);
    lerp(ix0, ix1, sy)
}
