use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::Path;

use crate::contracts::{
    ArtifactBundle, ManifestBounds, ModelManifest, StructuralIssue, StructuralMetrics,
    StructuralVerificationResult, VerifierSource, VerifierStatus,
};

const OVERHANG_NORMAL_Z_THRESHOLD: f32 = -0.70710677;
const BUILD_PLANE_EPSILON_MM: f32 = 0.001;

pub fn verify_structure(
    bundle: &ArtifactBundle,
    manifest: &ModelManifest,
) -> StructuralVerificationResult {
    let mut issues: Vec<StructuralIssue> = Vec::new();
    let mut preview_stl_size: Option<u64> = None;
    let mut preview_stl_triangle_count: Option<u32> = None;
    let mut preview_stl_component_count: Option<u32> = None;
    let mut preview_stl_non_manifold_edge_count: Option<u32> = None;
    let mut preview_stl_overhang_triangle_count: Option<u32> = None;
    let mut preview_stl_overhang_ratio: Option<f64> = None;

    // 1. Preview STL exists and is non-empty
    let stl_path = Path::new(&bundle.preview_stl_path);
    match fs::metadata(stl_path) {
        Ok(meta) => {
            let size = meta.len();
            preview_stl_size = Some(size);
            if size == 0 {
                issues.push(StructuralIssue {
                    code: "PREVIEW_STL_EMPTY".into(),
                    message: "Preview STL file is empty (0 bytes).".into(),
                    part_id: None,
                    numeric_payload: Some(0.0),
                });
            } else {
                match preview_stl_triangles(stl_path) {
                    Ok(StlPreview::Parsed(triangles)) if triangles.is_empty() => {
                        preview_stl_triangle_count = Some(0);
                        preview_stl_component_count = Some(0);
                        preview_stl_non_manifold_edge_count = Some(0);
                        preview_stl_overhang_triangle_count = Some(0);
                        preview_stl_overhang_ratio = Some(0.0);
                        issues.push(StructuralIssue {
                            code: "PREVIEW_STL_NO_TRIANGLES".into(),
                            message: "Preview STL file contains no triangles.".into(),
                            part_id: None,
                            numeric_payload: Some(0.0),
                        });
                    }
                    Ok(StlPreview::Parsed(triangles)) => {
                        let topology = preview_stl_topology_summary(&triangles);
                        preview_stl_triangle_count = Some(usize_metric(topology.triangle_count));
                        preview_stl_component_count = Some(usize_metric(topology.component_count));
                        preview_stl_non_manifold_edge_count =
                            Some(usize_metric(topology.non_manifold_edge_count));
                        preview_stl_overhang_triangle_count =
                            Some(usize_metric(topology.overhang_triangle_count));
                        preview_stl_overhang_ratio = Some(topology.overhang_ratio);
                        add_preview_stl_topology_issues(
                            &mut issues,
                            topology,
                            expected_preview_component_count(bundle, manifest),
                        );
                    }
                    Ok(StlPreview::Unreadable) | Err(_) => {
                        issues.push(StructuralIssue {
                            code: "PREVIEW_STL_UNREADABLE".into(),
                            message: "Preview STL file could not be parsed as valid STL.".into(),
                            part_id: None,
                            numeric_payload: None,
                        });
                    }
                }
            }
        }
        Err(_) => {
            issues.push(StructuralIssue {
                code: "PREVIEW_STL_MISSING".into(),
                message: format!("Preview STL file not found: {}", bundle.preview_stl_path),
                part_id: None,
                numeric_payload: None,
            });
        }
    }

    // 2. Manifest parts non-empty
    if manifest.parts.is_empty() {
        issues.push(StructuralIssue {
            code: "MANIFEST_PARTS_EMPTY".into(),
            message: "Manifest contains no parts.".into(),
            part_id: None,
            numeric_payload: None,
        });
    }

    let part_ids: HashSet<&str> = manifest.parts.iter().map(|p| p.part_id.as_str()).collect();

    // 3. Per-part checks
    for part in &manifest.parts {
        // Viewer asset path exists
        if let Some(ref asset_path) = part.viewer_asset_path {
            if !Path::new(asset_path).exists() {
                issues.push(StructuralIssue {
                    code: "PART_ASSET_MISSING".into(),
                    message: format!(
                        "Part '{}' viewer asset not found: {}",
                        part.label, asset_path
                    ),
                    part_id: Some(part.part_id.clone()),
                    numeric_payload: None,
                });
            }
        }

        // Bounds finite and non-degenerate
        if let Some(ref bounds) = part.bounds {
            if !bounds_valid(bounds) {
                issues.push(StructuralIssue {
                    code: "BOUNDS_DEGENERATE".into(),
                    message: format!("Part '{}' has degenerate or non-finite bounds.", part.label),
                    part_id: Some(part.part_id.clone()),
                    numeric_payload: None,
                });
            }
        }

        // Volume positive
        if let Some(vol) = part.volume {
            if !vol.is_finite() || vol <= 0.0 {
                issues.push(StructuralIssue {
                    code: "VOLUME_NON_POSITIVE".into(),
                    message: format!("Part '{}' has non-positive volume: {}", part.label, vol),
                    part_id: Some(part.part_id.clone()),
                    numeric_payload: Some(vol),
                });
            }
        }

        // Area positive
        if let Some(area) = part.area {
            if !area.is_finite() || area <= 0.0 {
                issues.push(StructuralIssue {
                    code: "AREA_NON_POSITIVE".into(),
                    message: format!(
                        "Part '{}' has non-positive surface area: {}",
                        part.label, area
                    ),
                    part_id: Some(part.part_id.clone()),
                    numeric_payload: Some(area),
                });
            }
        }
    }

    // 4. Assembly-level spatial checks (requires bounds data)
    {
        let parts_with_bounds: Vec<&crate::contracts::PartBinding> = manifest
            .parts
            .iter()
            .filter(|p| p.bounds.is_some())
            .collect();

        // GROUND_CONTACT_MISSING: whole assembly z_min > 10mm
        if !parts_with_bounds.is_empty() {
            let assembly_z_min = parts_with_bounds
                .iter()
                .map(|p| p.bounds.as_ref().unwrap().z_min)
                .fold(f64::INFINITY, f64::min);
            if assembly_z_min.is_finite() && assembly_z_min > 10.0 {
                issues.push(StructuralIssue {
                    code: "GROUND_CONTACT_MISSING".into(),
                    message: format!(
                        "Assembly base is {:.1}mm above z=0 — model may not be grounded on the build plate.",
                        assembly_z_min
                    ),
                    part_id: None,
                    numeric_payload: Some(assembly_z_min),
                });
            }
        }

        // Multipart-only checks
        if parts_with_bounds.len() >= 2 {
            // Find primary part (largest by volume, or first if no volume data)
            let primary_idx = manifest
                .parts
                .iter()
                .enumerate()
                .filter(|(_, p)| p.bounds.is_some())
                .max_by(|(_, a), (_, b)| {
                    a.volume
                        .unwrap_or(0.0)
                        .partial_cmp(&b.volume.unwrap_or(0.0))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(i, _)| i)
                .unwrap_or(0);
            let max_volume = manifest
                .parts
                .iter()
                .filter_map(|p| p.volume)
                .fold(0.0_f64, f64::max);

            for (idx, part) in manifest.parts.iter().enumerate() {
                let Some(ref bounds) = part.bounds else {
                    continue;
                };
                if idx == primary_idx {
                    continue;
                } // skip primary

                // PART_DISCONNECTED: min AABB distance to all other parts > 25mm
                let min_dist = manifest
                    .parts
                    .iter()
                    .enumerate()
                    .filter(|(j, p)| *j != idx && p.bounds.is_some())
                    .map(|(_, other)| aabb_distance(bounds, other.bounds.as_ref().unwrap()))
                    .fold(f64::INFINITY, f64::min);

                if min_dist > 25.0 {
                    issues.push(StructuralIssue {
                        code: "PART_DISCONNECTED".into(),
                        message: format!(
                            "Part '{}' is spatially isolated — nearest part is {:.1}mm away.",
                            part.label, min_dist
                        ),
                        part_id: Some(part.part_id.clone()),
                        numeric_payload: Some(min_dist),
                    });
                }

                // PART_TOO_SMALL: volume < 0.5% of max AND < 500mm³
                if let Some(vol) = part.volume {
                    if max_volume > 0.0 && vol > 0.0 && vol / max_volume < 0.005 && vol < 500.0 {
                        issues.push(StructuralIssue {
                            code: "PART_TOO_SMALL".into(),
                            message: format!(
                                "Part '{}' volume ({:.2}mm³) is suspiciously small — may be a degenerate fragment.",
                                part.label, vol
                            ),
                            part_id: Some(part.part_id.clone()),
                            numeric_payload: Some(vol),
                        });
                    }
                }
            }
        }
    }

    // 5. Multipart consistency: viewer assets reference known parts
    for asset in &bundle.viewer_assets {
        if !part_ids.contains(asset.part_id.as_str()) {
            issues.push(StructuralIssue {
                code: "VIEWER_ASSET_ORPHAN".into(),
                message: format!(
                    "Viewer asset '{}' references unknown part_id '{}'.",
                    asset.label, asset.part_id
                ),
                part_id: Some(asset.part_id.clone()),
                numeric_payload: None,
            });
        }
    }

    // Collect metrics
    let mut total_volume: Option<f64> = None;
    let mut total_area: Option<f64> = None;
    let mut merged_bbox: Option<ManifestBounds> = None;

    for part in &manifest.parts {
        if let Some(vol) = part.volume {
            *total_volume.get_or_insert(0.0) += vol;
        }
        if let Some(area) = part.area {
            *total_area.get_or_insert(0.0) += area;
        }
        if let Some(ref b) = part.bounds {
            merged_bbox = Some(match merged_bbox {
                None => b.clone(),
                Some(m) => ManifestBounds {
                    x_min: m.x_min.min(b.x_min),
                    y_min: m.y_min.min(b.y_min),
                    z_min: m.z_min.min(b.z_min),
                    x_max: m.x_max.max(b.x_max),
                    y_max: m.y_max.max(b.y_max),
                    z_max: m.z_max.max(b.z_max),
                },
            });
        }
    }

    let passed = issues.is_empty();
    let summary = if passed {
        "All structural checks passed.".into()
    } else {
        let codes: Vec<&str> = issues.iter().map(|i| i.code.as_str()).collect();
        format!("Structural verification failed: {}", codes.join(", "))
    };

    StructuralVerificationResult {
        passed,
        summary,
        issues,
        metrics: StructuralMetrics {
            part_count: manifest.parts.len() as u32,
            preview_stl_size_bytes: preview_stl_size,
            preview_stl_triangle_count,
            preview_stl_component_count,
            preview_stl_non_manifold_edge_count,
            preview_stl_overhang_triangle_count,
            preview_stl_overhang_ratio,
            total_volume,
            total_area,
            bbox: merged_bbox,
        },
        verifier_status: VerifierStatus::OkRustOnly,
        verifier_source: Some(VerifierSource::RustStructural),
    }
}

/// Minimum 3D distance between two axis-aligned bounding boxes.
/// Returns 0.0 when they overlap or touch.
fn aabb_distance(a: &ManifestBounds, b: &ManifestBounds) -> f64 {
    let dx = (a.x_min - b.x_max).max(b.x_min - a.x_max).max(0.0);
    let dy = (a.y_min - b.y_max).max(b.y_min - a.y_max).max(0.0);
    let dz = (a.z_min - b.z_max).max(b.z_min - a.z_max).max(0.0);
    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn bounds_valid(b: &ManifestBounds) -> bool {
    let vals = [b.x_min, b.y_min, b.z_min, b.x_max, b.y_max, b.z_max];
    if vals.iter().any(|v| !v.is_finite()) {
        return false;
    }
    // At least one axis must have min < max (non-degenerate)
    (b.x_min < b.x_max) || (b.y_min < b.y_max) || (b.z_min < b.z_max)
}

enum StlPreview {
    Parsed(Vec<StlTriangle>),
    Unreadable,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct StlVertex([u32; 3]);

#[derive(Clone, Copy, Debug)]
struct StlTriangle {
    vertices: [StlVertex; 3],
    coords: [[f32; 3]; 3],
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct StlTopologySummary {
    triangle_count: usize,
    component_count: usize,
    non_manifold_edge_count: usize,
    overhang_triangle_count: usize,
    overhang_ratio: f64,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct StlEdge {
    a: StlVertex,
    b: StlVertex,
}

impl StlVertex {
    fn new(coords: [f32; 3]) -> Self {
        Self(coords.map(stl_float_key))
    }
}

impl StlTriangle {
    fn edges(&self) -> [StlEdge; 3] {
        [
            StlEdge::new(self.vertices[0], self.vertices[1]),
            StlEdge::new(self.vertices[1], self.vertices[2]),
            StlEdge::new(self.vertices[2], self.vertices[0]),
        ]
    }
}

impl StlEdge {
    fn new(a: StlVertex, b: StlVertex) -> Self {
        if a <= b {
            Self { a, b }
        } else {
            Self { a: b, b: a }
        }
    }
}

fn stl_float_key(value: f32) -> u32 {
    if value == 0.0 {
        0.0_f32.to_bits()
    } else if value.is_nan() {
        f32::NAN.to_bits()
    } else {
        value.to_bits()
    }
}

fn preview_stl_triangles(path: &Path) -> std::io::Result<StlPreview> {
    let bytes = fs::read(path)?;
    let first_non_whitespace = bytes.iter().position(|b| !b.is_ascii_whitespace());

    if bytes.len() >= 84 {
        let triangle_count = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]);
        let expected_binary_len = (triangle_count as usize)
            .checked_mul(50)
            .and_then(|triangle_bytes| triangle_bytes.checked_add(84));
        if expected_binary_len == Some(bytes.len()) {
            return Ok(StlPreview::Parsed(parse_binary_stl_triangles(
                &bytes,
                triangle_count as usize,
            )));
        }
    }

    if let Some(first_non_whitespace) = first_non_whitespace {
        let stl_body = &bytes[first_non_whitespace..];
        if stl_body
            .get(..5)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"solid"))
        {
            return match std::str::from_utf8(stl_body) {
                Ok(text) => Ok(parse_ascii_stl_triangles(text)),
                Err(_) => Ok(StlPreview::Unreadable),
            };
        }
    }

    Ok(StlPreview::Unreadable)
}

fn parse_binary_stl_triangles(bytes: &[u8], triangle_count: usize) -> Vec<StlTriangle> {
    let mut triangles = Vec::with_capacity(triangle_count);
    let mut offset = 84;
    for _ in 0..triangle_count {
        offset += 12; // normal vector
        let mut coords = [[0.0_f32; 3]; 3];
        for vertex in &mut coords {
            let x = f32::from_le_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
            ]);
            offset += 4;
            let y = f32::from_le_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
            ]);
            offset += 4;
            let z = f32::from_le_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
            ]);
            offset += 4;
            *vertex = [x, y, z];
        }
        offset += 2; // attribute byte count
        triangles.push(StlTriangle {
            vertices: coords.map(StlVertex::new),
            coords,
        });
    }
    triangles
}

fn parse_ascii_stl_triangles(text: &str) -> StlPreview {
    let facet_count = ascii_stl_facet_count(text);
    if facet_count == 0 {
        return StlPreview::Parsed(Vec::new());
    }

    let mut triangles = Vec::with_capacity(facet_count);
    let mut current_vertices: Option<Vec<[f32; 3]>> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if starts_ascii_case(trimmed, "facet") {
            if current_vertices.is_some() {
                return StlPreview::Unreadable;
            }
            current_vertices = Some(Vec::with_capacity(3));
        } else if starts_ascii_case(trimmed, "vertex") {
            let Some(vertices) = current_vertices.as_mut() else {
                continue;
            };
            let Some(vertex) = parse_ascii_stl_vertex(trimmed) else {
                return StlPreview::Unreadable;
            };
            vertices.push(vertex);
        } else if starts_ascii_case(trimmed, "endfacet") {
            let Some(vertices) = current_vertices.take() else {
                continue;
            };
            let Ok(coords) = <Vec<[f32; 3]> as TryInto<[[f32; 3]; 3]>>::try_into(vertices) else {
                return StlPreview::Unreadable;
            };
            triangles.push(StlTriangle {
                vertices: coords.map(StlVertex::new),
                coords,
            });
        }
    }

    if current_vertices.is_some() || triangles.len() != facet_count {
        return StlPreview::Unreadable;
    }

    StlPreview::Parsed(triangles)
}

fn parse_ascii_stl_vertex(line: &str) -> Option<[f32; 3]> {
    let mut parts = line.split_whitespace();
    let label = parts.next()?;
    if !label.eq_ignore_ascii_case("vertex") {
        return None;
    }
    let x = parts.next()?.parse::<f32>().ok()?;
    let y = parts.next()?.parse::<f32>().ok()?;
    let z = parts.next()?.parse::<f32>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some([x, y, z])
}

fn ascii_stl_facet_count(text: &str) -> usize {
    text.lines()
        .filter(|line| {
            line.trim_start()
                .get(..5)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("facet"))
        })
        .count()
}

fn starts_ascii_case(text: &str, prefix: &str) -> bool {
    text.as_bytes()
        .get(..prefix.len())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix.as_bytes()))
}

fn preview_stl_topology_summary(triangles: &[StlTriangle]) -> StlTopologySummary {
    let edge_triangles = stl_edge_triangles(triangles);
    let non_manifold_edges = edge_triangles
        .values()
        .filter(|triangle_ids| triangle_ids.len() != 2)
        .count();
    let component_count = stl_component_count(triangles.len(), &edge_triangles);
    let overhang_triangle_count = stl_overhang_triangle_count(triangles);
    let overhang_ratio = if triangles.is_empty() {
        0.0
    } else {
        overhang_triangle_count as f64 / triangles.len() as f64
    };
    StlTopologySummary {
        triangle_count: triangles.len(),
        component_count,
        non_manifold_edge_count: non_manifold_edges,
        overhang_triangle_count,
        overhang_ratio,
    }
}

fn add_preview_stl_topology_issues(
    issues: &mut Vec<StructuralIssue>,
    topology: StlTopologySummary,
    expected_component_count: usize,
) {
    let non_manifold_edges = topology.non_manifold_edge_count;
    if non_manifold_edges > 0 {
        issues.push(StructuralIssue {
            code: "PREVIEW_STL_NON_MANIFOLD".into(),
            message: format!(
                "Preview STL contains {} non-manifold edge(s).",
                non_manifold_edges
            ),
            part_id: None,
            numeric_payload: Some(non_manifold_edges as f64),
        });
    }

    let component_count = topology.component_count;
    if component_count > expected_component_count {
        issues.push(StructuralIssue {
            code: "PREVIEW_STL_DISCONNECTED_COMPONENTS".into(),
            message: format!(
                "Preview STL contains {} disconnected triangle components.",
                component_count
            ),
            part_id: None,
            numeric_payload: Some(component_count as f64),
        });
    }
}

fn expected_preview_component_count(bundle: &ArtifactBundle, manifest: &ModelManifest) -> usize {
    let viewer_part_count = bundle
        .viewer_assets
        .iter()
        .map(|asset| asset.part_id.as_str())
        .collect::<HashSet<_>>()
        .len();
    viewer_part_count.max(manifest.parts.len()).max(1)
}

fn usize_metric(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn stl_overhang_triangle_count(triangles: &[StlTriangle]) -> usize {
    let Some(min_z) = stl_min_z(triangles) else {
        return 0;
    };
    triangles
        .iter()
        .filter(|triangle| {
            let centroid_z = triangle.coords.iter().map(|p| p[2]).sum::<f32>() / 3.0;
            if centroid_z <= min_z + BUILD_PLANE_EPSILON_MM {
                return false;
            }
            triangle_unit_normal_z(triangle)
                .is_some_and(|normal_z| normal_z <= OVERHANG_NORMAL_Z_THRESHOLD)
        })
        .count()
}

fn stl_min_z(triangles: &[StlTriangle]) -> Option<f32> {
    triangles
        .iter()
        .flat_map(|triangle| triangle.coords.iter().map(|point| point[2]))
        .filter(|z| z.is_finite())
        .reduce(f32::min)
}

fn triangle_unit_normal_z(triangle: &StlTriangle) -> Option<f32> {
    let [a, b, c] = triangle.coords;
    let ux = b[0] - a[0];
    let uy = b[1] - a[1];
    let uz = b[2] - a[2];
    let vx = c[0] - a[0];
    let vy = c[1] - a[1];
    let vz = c[2] - a[2];
    let nx = uy * vz - uz * vy;
    let ny = uz * vx - ux * vz;
    let nz = ux * vy - uy * vx;
    let len = (nx * nx + ny * ny + nz * nz).sqrt();
    if !len.is_finite() || len <= f32::EPSILON {
        return None;
    }
    Some(nz / len)
}

fn stl_edge_triangles(triangles: &[StlTriangle]) -> HashMap<StlEdge, Vec<usize>> {
    let mut edge_triangles: HashMap<StlEdge, Vec<usize>> = HashMap::new();
    for (triangle_idx, triangle) in triangles.iter().enumerate() {
        for edge in triangle.edges() {
            edge_triangles.entry(edge).or_default().push(triangle_idx);
        }
    }
    edge_triangles
}

fn stl_component_count(
    triangle_count: usize,
    edge_triangles: &HashMap<StlEdge, Vec<usize>>,
) -> usize {
    let mut adjacency = vec![Vec::new(); triangle_count];
    for triangle_ids in edge_triangles.values() {
        for (position, &left) in triangle_ids.iter().enumerate() {
            for &right in &triangle_ids[(position + 1)..] {
                if left == right {
                    continue;
                }
                adjacency[left].push(right);
                adjacency[right].push(left);
            }
        }
    }

    let mut visited = vec![false; triangle_count];
    let mut components = 0;
    for start in 0..triangle_count {
        if visited[start] {
            continue;
        }
        components += 1;
        visited[start] = true;
        let mut queue = VecDeque::from([start]);
        while let Some(current) = queue.pop_front() {
            for &next in &adjacency[current] {
                if visited[next] {
                    continue;
                }
                visited[next] = true;
                queue.push_back(next);
            }
        }
    }
    components
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::*;
    use std::io::Write;

    fn test_bundle(dir: &Path) -> ArtifactBundle {
        let stl_path = dir.join("preview.stl");
        write_closed_tetra_binary_stl(&stl_path, 0.0);

        let manifest_path = dir.join("manifest.json");
        fs::write(&manifest_path, "{}").unwrap();

        ArtifactBundle {
            schema_version: 1,
            model_id: "generated-test-001".into(),
            source_kind: ModelSourceKind::Generated,
            engine_kind: EngineKind::Freecad,
            source_language: SourceLanguage::LegacyPython,
            geometry_backend: GeometryBackend::Freecad,
            content_hash: "abc123".into(),
            artifact_version: 1,
            fcstd_path: dir.join("model.fcstd").to_string_lossy().into(),
            manifest_path: manifest_path.to_string_lossy().into(),
            macro_path: None,
            preview_stl_path: stl_path.to_string_lossy().into(),
            viewer_assets: vec![],
            edge_targets: vec![],
            face_targets: vec![],
            callout_anchors: vec![],
            measurement_guides: vec![],
            export_artifacts: vec![],
        }
    }

    fn write_zero_triangle_binary_stl(path: &Path) {
        let mut f = fs::File::create(path).unwrap();
        f.write_all(&[0u8; 80]).unwrap();
        f.write_all(&0u32.to_le_bytes()).unwrap();
        f.flush().unwrap();
    }

    fn write_one_triangle_binary_stl(path: &Path) {
        write_binary_stl(path, &[[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]]);
    }

    fn write_raised_downward_triangle_binary_stl(path: &Path) {
        write_binary_stl(
            path,
            &[
                [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                [[0.0, 0.0, 2.0], [0.0, 1.0, 2.0], [1.0, 0.0, 2.0]],
            ],
        );
    }

    fn write_closed_tetra_binary_stl(path: &Path, x_offset: f32) {
        let triangles = closed_tetra_triangles(x_offset);
        write_binary_stl(path, &triangles);
    }

    fn closed_tetra_triangles(x_offset: f32) -> Vec<[[f32; 3]; 3]> {
        let a = [x_offset, 0.0, 0.0];
        let b = [x_offset + 1.0, 0.0, 0.0];
        let c = [x_offset, 1.0, 0.0];
        let d = [x_offset, 0.0, 1.0];
        vec![[a, b, c], [a, d, b], [a, c, d], [b, d, c]]
    }

    fn write_two_tetra_binary_stl(path: &Path) {
        let mut triangles = closed_tetra_triangles(0.0);
        triangles.extend(closed_tetra_triangles(10.0));
        write_binary_stl(path, &triangles);
    }

    fn write_binary_stl(path: &Path, triangles: &[[[f32; 3]; 3]]) {
        let mut f = fs::File::create(path).unwrap();
        f.write_all(&[0u8; 80]).unwrap();
        f.write_all(&(triangles.len() as u32).to_le_bytes())
            .unwrap();
        for triangle in triangles {
            f.write_all(&[0u8; 12]).unwrap();
            for vertex in triangle {
                for coordinate in vertex {
                    f.write_all(&coordinate.to_le_bytes()).unwrap();
                }
            }
            f.write_all(&[0u8; 2]).unwrap();
        }
        f.flush().unwrap();
    }

    fn write_ascii_stl(path: &Path, triangles: &[[[f32; 3]; 3]]) {
        let mut text = String::from("solid preview\n");
        for triangle in triangles {
            text.push_str("  facet normal 0 0 0\n");
            text.push_str("    outer loop\n");
            for [x, y, z] in triangle {
                text.push_str(&format!("      vertex {x} {y} {z}\n"));
            }
            text.push_str("    endloop\n");
            text.push_str("  endfacet\n");
        }
        text.push_str("endsolid preview\n");
        fs::write(path, text).unwrap();
    }

    fn write_binary_stl_with_declared_triangle_count(path: &Path, triangle_count: u32) {
        let mut f = fs::File::create(path).unwrap();
        f.write_all(&[0u8; 80]).unwrap();
        f.write_all(&triangle_count.to_le_bytes()).unwrap();
        f.flush().unwrap();
    }

    fn test_manifest() -> ModelManifest {
        ModelManifest {
            schema_version: 1,
            model_id: "generated-test-001".into(),
            source_kind: ModelSourceKind::Generated,
            source_digest: None,
            core_digest: None,
            ast_schema_version: None,
            engine_kind: EngineKind::Freecad,
            source_language: SourceLanguage::LegacyPython,
            geometry_backend: GeometryBackend::Freecad,
            document: DocumentMetadata {
                document_name: "Test".into(),
                document_label: "Test Model".into(),
                source_path: None,
                object_count: 1,
                warnings: vec![],
            },
            parts: vec![PartBinding {
                part_id: "part-1".into(),
                freecad_object_name: "Body".into(),
                label: "Main Body".into(),
                kind: "solid".into(),
                semantic_role: None,
                viewer_asset_path: None,
                viewer_node_ids: vec![],
                parameter_keys: vec![],
                editable: true,
                bounds: Some(ManifestBounds {
                    x_min: -10.0,
                    y_min: -10.0,
                    z_min: 0.0,
                    x_max: 10.0,
                    y_max: 10.0,
                    z_max: 20.0,
                }),
                volume: Some(1000.0),
                area: Some(600.0),
            }],
            parameter_groups: vec![],
            control_primitives: vec![],
            control_relations: vec![],
            control_views: vec![],
            advisories: vec![],
            selection_targets: vec![],
            measurement_annotations: vec![],
            feature_graph: None,
            correspondence_graph: None,
            warnings: vec![],
            enrichment_state: ManifestEnrichmentState {
                status: EnrichmentStatus::None,
                proposals: vec![],
            },
        }
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ecky-sv-test-{}-{}", std::process::id(), name));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn valid_bundle_passes() {
        let dir = temp_dir("valid");
        let bundle = test_bundle(&dir);
        let manifest = test_manifest();
        let result = verify_structure(&bundle, &manifest);
        assert!(result.passed, "Expected pass, got: {:?}", result.issues);
        assert_eq!(result.verifier_status, VerifierStatus::OkRustOnly);
        assert_eq!(result.verifier_source, Some(VerifierSource::RustStructural));
        assert_eq!(result.metrics.part_count, 1);
        assert!(result.metrics.preview_stl_size_bytes.unwrap() > 0);
        assert!((result.metrics.total_volume.unwrap() - 1000.0).abs() < f64::EPSILON);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn closed_tetra_preview_stl_has_no_topology_issues() {
        let dir = temp_dir("closed_tetra");
        let bundle = test_bundle(&dir);
        write_closed_tetra_binary_stl(Path::new(&bundle.preview_stl_path), 0.0);
        let manifest = test_manifest();
        let result = verify_structure(&bundle, &manifest);
        assert!(result.passed, "Expected pass, got: {:?}", result.issues);
        assert_eq!(result.metrics.preview_stl_triangle_count, Some(4));
        assert_eq!(result.metrics.preview_stl_component_count, Some(1));
        assert_eq!(result.metrics.preview_stl_non_manifold_edge_count, Some(0));
        assert_eq!(result.metrics.preview_stl_overhang_triangle_count, Some(0));
        assert_eq!(result.metrics.preview_stl_overhang_ratio, Some(0.0));
        assert!(!result
            .issues
            .iter()
            .any(|i| i.code == "PREVIEW_STL_NON_MANIFOLD"));
        assert!(!result
            .issues
            .iter()
            .any(|i| i.code == "PREVIEW_STL_DISCONNECTED_COMPONENTS"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn open_single_triangle_preview_stl_fails_non_manifold() {
        let dir = temp_dir("binary_one_triangle");
        let bundle = test_bundle(&dir);
        write_one_triangle_binary_stl(Path::new(&bundle.preview_stl_path));
        let manifest = test_manifest();
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(!result
            .issues
            .iter()
            .any(|i| i.code == "PREVIEW_STL_NO_TRIANGLES"));
        assert!(!result
            .issues
            .iter()
            .any(|i| i.code == "PREVIEW_STL_UNREADABLE"));
        assert!(result
            .issues
            .iter()
            .any(|i| i.code == "PREVIEW_STL_NON_MANIFOLD"));
        assert_eq!(result.metrics.preview_stl_triangle_count, Some(1));
        assert_eq!(result.metrics.preview_stl_component_count, Some(1));
        assert_eq!(result.metrics.preview_stl_non_manifold_edge_count, Some(3));
        assert_eq!(result.metrics.preview_stl_overhang_triangle_count, Some(0));
        assert_eq!(result.metrics.preview_stl_overhang_ratio, Some(0.0));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn raised_downward_preview_stl_reports_overhang_metric_without_failing_for_overhang() {
        let dir = temp_dir("raised_downward_triangle");
        let bundle = test_bundle(&dir);
        write_raised_downward_triangle_binary_stl(Path::new(&bundle.preview_stl_path));
        let manifest = test_manifest();
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(result
            .issues
            .iter()
            .any(|i| i.code == "PREVIEW_STL_NON_MANIFOLD"));
        assert!(!result
            .issues
            .iter()
            .any(|i| i.code == "PREVIEW_STL_OVERHANG_RISK"));
        assert_eq!(result.metrics.preview_stl_triangle_count, Some(2));
        assert_eq!(result.metrics.preview_stl_overhang_triangle_count, Some(1));
        assert_eq!(result.metrics.preview_stl_overhang_ratio, Some(0.5));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ascii_open_single_triangle_preview_stl_fails_non_manifold() {
        let dir = temp_dir("ascii_one_triangle");
        let bundle = test_bundle(&dir);
        write_ascii_stl(
            Path::new(&bundle.preview_stl_path),
            &[[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]],
        );
        let manifest = test_manifest();
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(result
            .issues
            .iter()
            .any(|i| i.code == "PREVIEW_STL_NON_MANIFOLD"));
        assert!(!result
            .issues
            .iter()
            .any(|i| i.code == "PREVIEW_STL_UNREADABLE"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn separated_tetra_preview_stl_fails_disconnected_components() {
        let dir = temp_dir("two_tetra");
        let bundle = test_bundle(&dir);
        write_two_tetra_binary_stl(Path::new(&bundle.preview_stl_path));
        let manifest = test_manifest();
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        let issue = result
            .issues
            .iter()
            .find(|i| i.code == "PREVIEW_STL_DISCONNECTED_COMPONENTS")
            .expect("expected disconnected component issue");
        assert_eq!(issue.numeric_payload, Some(2.0));
        assert_eq!(result.metrics.preview_stl_triangle_count, Some(8));
        assert_eq!(result.metrics.preview_stl_component_count, Some(2));
        assert_eq!(result.metrics.preview_stl_non_manifold_edge_count, Some(0));
        assert_eq!(result.metrics.preview_stl_overhang_triangle_count, Some(0));
        assert_eq!(result.metrics.preview_stl_overhang_ratio, Some(0.0));
        assert!(!result
            .issues
            .iter()
            .any(|i| i.code == "PREVIEW_STL_NON_MANIFOLD"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn multipart_preview_stl_allows_one_component_per_part() {
        let dir = temp_dir("multipart_two_tetra");
        let mut bundle = test_bundle(&dir);
        write_two_tetra_binary_stl(Path::new(&bundle.preview_stl_path));
        bundle.viewer_assets = vec![
            ViewerAsset {
                part_id: "part-1".into(),
                node_id: "part-1".into(),
                object_name: "Part1".into(),
                label: "Part 1".into(),
                path: "part-1.stl".into(),
                format: ViewerAssetFormat::Stl,
            },
            ViewerAsset {
                part_id: "part-2".into(),
                node_id: "part-2".into(),
                object_name: "Part2".into(),
                label: "Part 2".into(),
                path: "part-2.stl".into(),
                format: ViewerAssetFormat::Stl,
            },
        ];
        let mut manifest = test_manifest();
        let mut second = manifest.parts[0].clone();
        second.part_id = "part-2".into();
        second.freecad_object_name = "Body2".into();
        second.label = "Second Body".into();
        manifest.parts.push(second);

        let result = verify_structure(&bundle, &manifest);

        assert!(result.passed, "Expected pass, got: {:?}", result.issues);
        assert_eq!(result.metrics.preview_stl_component_count, Some(2));
        assert_eq!(result.metrics.preview_stl_non_manifold_edge_count, Some(0));
        assert!(!result
            .issues
            .iter()
            .any(|i| i.code == "PREVIEW_STL_DISCONNECTED_COMPONENTS"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn binary_preview_stl_without_triangles_fails() {
        let dir = temp_dir("binary_empty_mesh");
        let bundle = test_bundle(&dir);
        write_zero_triangle_binary_stl(Path::new(&bundle.preview_stl_path));
        let manifest = test_manifest();
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(result
            .issues
            .iter()
            .any(|i| i.code == "PREVIEW_STL_NO_TRIANGLES"));
        assert_eq!(result.metrics.preview_stl_triangle_count, Some(0));
        assert_eq!(result.metrics.preview_stl_component_count, Some(0));
        assert_eq!(result.metrics.preview_stl_non_manifold_edge_count, Some(0));
        assert_eq!(result.metrics.preview_stl_overhang_triangle_count, Some(0));
        assert_eq!(result.metrics.preview_stl_overhang_ratio, Some(0.0));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn truncated_binary_preview_stl_fails_unreadable() {
        let dir = temp_dir("binary_truncated");
        let bundle = test_bundle(&dir);
        fs::write(&bundle.preview_stl_path, [0u8; 83]).unwrap();
        let manifest = test_manifest();
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(result
            .issues
            .iter()
            .any(|i| i.code == "PREVIEW_STL_UNREADABLE"));
        assert!(!result
            .issues
            .iter()
            .any(|i| i.code == "PREVIEW_STL_NO_TRIANGLES"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn binary_preview_stl_count_mismatch_fails_unreadable() {
        let dir = temp_dir("binary_count_mismatch");
        let bundle = test_bundle(&dir);
        write_binary_stl_with_declared_triangle_count(Path::new(&bundle.preview_stl_path), 2);
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(&bundle.preview_stl_path)
            .unwrap();
        f.write_all(&[0u8; 50]).unwrap();
        f.flush().unwrap();
        let manifest = test_manifest();
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(result
            .issues
            .iter()
            .any(|i| i.code == "PREVIEW_STL_UNREADABLE"));
        assert!(!result
            .issues
            .iter()
            .any(|i| i.code == "PREVIEW_STL_NO_TRIANGLES"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ascii_preview_stl_without_facets_fails() {
        let dir = temp_dir("ascii_empty_mesh");
        let bundle = test_bundle(&dir);
        fs::write(&bundle.preview_stl_path, b"solid empty\nendsolid empty\n").unwrap();
        let manifest = test_manifest();
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(result
            .issues
            .iter()
            .any(|i| i.code == "PREVIEW_STL_NO_TRIANGLES"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn malformed_ascii_preview_stl_without_facets_fails_no_triangles() {
        let dir = temp_dir("ascii_malformed_empty_mesh");
        let bundle = test_bundle(&dir);
        fs::write(
            &bundle.preview_stl_path,
            b"solid malformed\nvertex nonsense without facet\nendsolid malformed\n",
        )
        .unwrap();
        let manifest = test_manifest();
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(result
            .issues
            .iter()
            .any(|i| i.code == "PREVIEW_STL_NO_TRIANGLES"));
        assert!(!result
            .issues
            .iter()
            .any(|i| i.code == "PREVIEW_STL_UNREADABLE"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn invalid_utf8_ascii_preview_stl_fails_unreadable() {
        let dir = temp_dir("ascii_invalid_utf8");
        let bundle = test_bundle(&dir);
        fs::write(
            &bundle.preview_stl_path,
            b"solid invalid\n\xff\nendsolid invalid\n",
        )
        .unwrap();
        let manifest = test_manifest();
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(result
            .issues
            .iter()
            .any(|i| i.code == "PREVIEW_STL_UNREADABLE"));
        assert!(!result
            .issues
            .iter()
            .any(|i| i.code == "PREVIEW_STL_NO_TRIANGLES"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_preview_stl_fails() {
        let dir = temp_dir("missing_stl");
        let mut bundle = test_bundle(&dir);
        bundle.preview_stl_path = dir.join("nonexistent.stl").to_string_lossy().into();
        let manifest = test_manifest();
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(result
            .issues
            .iter()
            .any(|i| i.code == "PREVIEW_STL_MISSING"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_preview_stl_fails() {
        let dir = temp_dir("empty_stl");
        let bundle = test_bundle(&dir);
        // Overwrite with empty file
        fs::write(&bundle.preview_stl_path, b"").unwrap();
        let manifest = test_manifest();
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(result.issues.iter().any(|i| i.code == "PREVIEW_STL_EMPTY"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_manifest_parts_fails() {
        let dir = temp_dir("empty_parts");
        let bundle = test_bundle(&dir);
        let mut manifest = test_manifest();
        manifest.parts.clear();
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(result
            .issues
            .iter()
            .any(|i| i.code == "MANIFEST_PARTS_EMPTY"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_part_asset_fails() {
        let dir = temp_dir("missing_asset");
        let bundle = test_bundle(&dir);
        let mut manifest = test_manifest();
        manifest.parts[0].viewer_asset_path =
            Some(dir.join("missing-part.stl").to_string_lossy().into());
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(result.issues.iter().any(|i| i.code == "PART_ASSET_MISSING"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn degenerate_bounds_fails() {
        let dir = temp_dir("degen_bounds");
        let bundle = test_bundle(&dir);
        let mut manifest = test_manifest();
        manifest.parts[0].bounds = Some(ManifestBounds {
            x_min: 5.0,
            y_min: 5.0,
            z_min: 5.0,
            x_max: 5.0, // all axes degenerate
            y_max: 5.0,
            z_max: 5.0,
        });
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(result.issues.iter().any(|i| i.code == "BOUNDS_DEGENERATE"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn non_positive_volume_fails() {
        let dir = temp_dir("neg_vol");
        let bundle = test_bundle(&dir);
        let mut manifest = test_manifest();
        manifest.parts[0].volume = Some(-5.0);
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(result
            .issues
            .iter()
            .any(|i| i.code == "VOLUME_NON_POSITIVE"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn non_positive_area_fails() {
        let dir = temp_dir("zero_area");
        let bundle = test_bundle(&dir);
        let mut manifest = test_manifest();
        manifest.parts[0].area = Some(0.0);
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(result.issues.iter().any(|i| i.code == "AREA_NON_POSITIVE"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn orphan_viewer_asset_fails() {
        let dir = temp_dir("orphan");
        let mut bundle = test_bundle(&dir);
        bundle.viewer_assets.push(ViewerAsset {
            part_id: "unknown-part".into(),
            node_id: "node-1".into(),
            object_name: "Ghost".into(),
            label: "Ghost Part".into(),
            path: dir.join("ghost.stl").to_string_lossy().into(),
            format: ViewerAssetFormat::Stl,
        });
        let manifest = test_manifest();
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(result
            .issues
            .iter()
            .any(|i| i.code == "VIEWER_ASSET_ORPHAN"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn nan_bounds_fails() {
        let dir = temp_dir("nan_bounds");
        let bundle = test_bundle(&dir);
        let mut manifest = test_manifest();
        manifest.parts[0].bounds = Some(ManifestBounds {
            x_min: f64::NAN,
            y_min: 0.0,
            z_min: 0.0,
            x_max: 10.0,
            y_max: 10.0,
            z_max: 10.0,
        });
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(result.issues.iter().any(|i| i.code == "BOUNDS_DEGENERATE"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn metrics_merge_multiple_parts() {
        let dir = temp_dir("merge");
        let bundle = test_bundle(&dir);
        let mut manifest = test_manifest();
        manifest.parts.push(PartBinding {
            part_id: "part-2".into(),
            freecad_object_name: "Body2".into(),
            label: "Second Part".into(),
            kind: "solid".into(),
            semantic_role: None,
            viewer_asset_path: None,
            viewer_node_ids: vec![],
            parameter_keys: vec![],
            editable: true,
            bounds: Some(ManifestBounds {
                x_min: 20.0,
                y_min: 20.0,
                z_min: 0.0,
                x_max: 30.0,
                y_max: 30.0,
                z_max: 15.0,
            }),
            volume: Some(500.0),
            area: Some(300.0),
        });
        let result = verify_structure(&bundle, &manifest);
        assert!(result.passed);
        assert_eq!(result.metrics.part_count, 2);
        assert!((result.metrics.total_volume.unwrap() - 1500.0).abs() < f64::EPSILON);
        assert!((result.metrics.total_area.unwrap() - 900.0).abs() < f64::EPSILON);
        let bbox = result.metrics.bbox.unwrap();
        assert!((bbox.x_min - (-10.0)).abs() < f64::EPSILON);
        assert!((bbox.x_max - 30.0).abs() < f64::EPSILON);
        fs::remove_dir_all(&dir).ok();
    }

    // ── Assembly-level checks ────────────────────────────────────────────────

    #[test]
    fn floating_assembly_triggers_ground_contact_missing() {
        let dir = temp_dir("gnd_miss");
        let bundle = test_bundle(&dir);
        let mut manifest = test_manifest();
        // Move the only part to z=50..70 — well above ground
        manifest.parts[0].bounds = Some(ManifestBounds {
            x_min: -10.0,
            y_min: -10.0,
            z_min: 50.0,
            x_max: 10.0,
            y_max: 10.0,
            z_max: 70.0,
        });
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(
            result
                .issues
                .iter()
                .any(|i| i.code == "GROUND_CONTACT_MISSING"),
            "expected GROUND_CONTACT_MISSING, got: {:?}",
            result.issues.iter().map(|i| &i.code).collect::<Vec<_>>()
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn grounded_assembly_passes_ground_contact() {
        let dir = temp_dir("gnd_ok");
        let bundle = test_bundle(&dir);
        let manifest = test_manifest(); // default z_min=0
        let result = verify_structure(&bundle, &manifest);
        assert!(result.passed);
        assert!(!result
            .issues
            .iter()
            .any(|i| i.code == "GROUND_CONTACT_MISSING"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn detached_secondary_part_triggers_part_disconnected() {
        let dir = temp_dir("disconn");
        let bundle = test_bundle(&dir);
        let mut manifest = test_manifest();
        // Primary part: x:-10..10, secondary 100mm away in X
        manifest.parts.push(PartBinding {
            part_id: "secondary".into(),
            freecad_object_name: "Secondary".into(),
            label: "Secondary".into(),
            kind: "solid".into(),
            semantic_role: None,
            viewer_asset_path: None,
            viewer_node_ids: vec![],
            parameter_keys: vec![],
            editable: true,
            bounds: Some(ManifestBounds {
                x_min: 100.0,
                y_min: -5.0,
                z_min: 0.0,
                x_max: 110.0,
                y_max: 5.0,
                z_max: 10.0,
            }),
            volume: Some(500.0),
            area: Some(100.0),
        });
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(
            result.issues.iter().any(|i| i.code == "PART_DISCONNECTED"),
            "expected PART_DISCONNECTED, got: {:?}",
            result.issues.iter().map(|i| &i.code).collect::<Vec<_>>()
        );
        assert!(result
            .issues
            .iter()
            .any(|i| i.code == "PART_DISCONNECTED" && i.part_id.as_deref() == Some("secondary")));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn adjacent_parts_do_not_trigger_disconnected() {
        let dir = temp_dir("adj_ok");
        let bundle = test_bundle(&dir);
        let mut manifest = test_manifest(); // primary: x:-10..10
        manifest.parts.push(PartBinding {
            part_id: "secondary".into(),
            freecad_object_name: "Secondary".into(),
            label: "Secondary".into(),
            kind: "solid".into(),
            semantic_role: None,
            viewer_asset_path: None,
            viewer_node_ids: vec![],
            parameter_keys: vec![],
            editable: true,
            bounds: Some(ManifestBounds {
                x_min: 10.0,
                y_min: -5.0,
                z_min: 0.0,
                x_max: 20.0,
                y_max: 5.0,
                z_max: 10.0,
            }),
            volume: Some(500.0),
            area: Some(100.0),
        });
        let result = verify_structure(&bundle, &manifest);
        assert!(
            !result.issues.iter().any(|i| i.code == "PART_DISCONNECTED"),
            "unexpected PART_DISCONNECTED: {:?}",
            result.issues
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn degenerate_tiny_secondary_triggers_part_too_small() {
        let dir = temp_dir("tiny");
        let bundle = test_bundle(&dir);
        let mut manifest = test_manifest(); // primary vol=1000
        manifest.parts.push(PartBinding {
            part_id: "tiny-part".into(),
            freecad_object_name: "Tiny".into(),
            label: "Tiny Part".into(),
            kind: "solid".into(),
            semantic_role: None,
            viewer_asset_path: None,
            viewer_node_ids: vec![],
            parameter_keys: vec![],
            editable: true,
            bounds: Some(ManifestBounds {
                x_min: 0.0,
                y_min: 0.0,
                z_min: 0.0,
                x_max: 1.0,
                y_max: 1.0,
                z_max: 1.0,
            }),
            volume: Some(0.5), // 0.05% of 1000, way below 0.5% threshold
            area: Some(6.0),
        });
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(
            result.issues.iter().any(|i| i.code == "PART_TOO_SMALL"),
            "expected PART_TOO_SMALL, got: {:?}",
            result.issues.iter().map(|i| &i.code).collect::<Vec<_>>()
        );
        assert!(result
            .issues
            .iter()
            .any(|i| i.code == "PART_TOO_SMALL" && i.part_id.as_deref() == Some("tiny-part")));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reasonable_secondary_part_passes_size_check() {
        let dir = temp_dir("size_ok");
        let bundle = test_bundle(&dir);
        let mut manifest = test_manifest(); // primary vol=1000
        manifest.parts.push(PartBinding {
            part_id: "secondary".into(),
            freecad_object_name: "Secondary".into(),
            label: "Secondary".into(),
            kind: "solid".into(),
            semantic_role: None,
            viewer_asset_path: None,
            viewer_node_ids: vec![],
            parameter_keys: vec![],
            editable: true,
            bounds: Some(ManifestBounds {
                x_min: 5.0,
                y_min: 5.0,
                z_min: 0.0,
                x_max: 10.0,
                y_max: 10.0,
                z_max: 10.0,
            }),
            volume: Some(100.0), // 10% of 1000 — fine
            area: Some(50.0),
        });
        let result = verify_structure(&bundle, &manifest);
        assert!(
            !result.issues.iter().any(|i| i.code == "PART_TOO_SMALL"),
            "unexpected PART_TOO_SMALL: {:?}",
            result.issues
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn single_part_model_skips_multipart_assembly_checks() {
        let dir = temp_dir("single");
        let bundle = test_bundle(&dir);
        let manifest = test_manifest(); // 1 part only
        let result = verify_structure(&bundle, &manifest);
        assert!(
            !result
                .issues
                .iter()
                .any(|i| i.code == "PART_DISCONNECTED" || i.code == "PART_TOO_SMALL"),
            "single-part model should not trigger multipart checks: {:?}",
            result.issues
        );
        fs::remove_dir_all(&dir).ok();
    }
}
