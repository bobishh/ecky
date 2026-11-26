use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::contracts::{AppError, AppResult, SelectionTarget};

pub const ANALYSIS_BOUNDARY_WELD_TOLERANCE_MM: f64 = 1.0e-6;
const ANALYSIS_BOUNDARY_SCHEMA_VERSION: u32 = 1;
const ANALYSIS_BOUNDARY_FACE_AREA_RELATIVE_TOLERANCE: f64 = 0.025;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct AnalysisBoundaryTopologyReport {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    tessellation_policy: AnalysisBoundaryTessellationPolicy,
    #[serde(default)]
    parts: Vec<AnalysisBoundaryTopologyPart>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisBoundaryTessellationPolicy {
    pub linear_deflection_mm: f64,
    pub angular_deflection_rad: f64,
}

impl Default for AnalysisBoundaryTessellationPolicy {
    fn default() -> Self {
        Self {
            linear_deflection_mm: 0.04,
            angular_deflection_rad: 0.1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct AnalysisBoundaryTopologyPart {
    part_id: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    representation: String,
    #[serde(default)]
    source_geometry_digest: Option<String>,
    #[serde(default)]
    triangles: Vec<AnalysisBoundaryTriangle>,
    #[serde(default)]
    triangle_face_group_indices: Vec<u32>,
    #[serde(default)]
    vertices: Vec<AnalysisBoundaryTopologyVertex>,
    #[serde(default)]
    edges: Vec<AnalysisBoundaryTopologyEdge>,
    #[serde(default)]
    faces: Vec<AnalysisBoundaryTopologyFace>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct AnalysisBoundaryTopologyVertex {
    target_id: Option<String>,
    #[serde(default)]
    point: Option<AnalysisBoundaryPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct AnalysisBoundaryTopologyEdge {
    target_id: Option<String>,
    #[serde(default)]
    vertex_target_ids: Vec<String>,
    #[serde(default)]
    start: Option<AnalysisBoundaryPoint>,
    #[serde(default)]
    end: Option<AnalysisBoundaryPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct AnalysisBoundaryTopologyFace {
    target_id: Option<String>,
    #[serde(default)]
    face_index: Option<u32>,
    #[serde(default)]
    label: String,
    #[serde(default)]
    center: Option<AnalysisBoundaryPoint>,
    #[serde(default)]
    normal: Option<[f64; 3]>,
    #[serde(default)]
    area: Option<f64>,
    #[serde(default)]
    boundary_edge_target_ids: Vec<Vec<String>>,
    #[serde(default)]
    exact_geometry: Option<AnalysisBoundaryTopologyFaceGeometry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct AnalysisBoundaryTopologyFaceGeometry {
    #[serde(default)]
    boundary_edge_target_ids: Vec<Vec<String>>,
}

impl AnalysisBoundaryTopologyFace {
    fn boundary_loops(&self) -> &[Vec<String>] {
        if !self.boundary_edge_target_ids.is_empty() {
            &self.boundary_edge_target_ids
        } else {
            self.exact_geometry
                .as_ref()
                .map(|geometry| geometry.boundary_edge_target_ids.as_slice())
                .unwrap_or(&[])
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct AnalysisBoundaryPoint {
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct AnalysisBoundaryTriangle {
    vertices: [[f64; 3]; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct AnalysisBoundaryManifestView {
    #[serde(default)]
    selection_targets: Vec<SelectionTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisBoundaryFaceGroup {
    pub part_id: String,
    pub target_id: String,
    pub canonical_target_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub durable_target_id: Option<String>,
    pub label: String,
    pub area: f64,
    pub triangle_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisBoundaryEvidence {
    pub closed: bool,
    pub manifold: bool,
    pub component_count: usize,
    pub positive_volume: bool,
    pub boundary_edge_count: usize,
    pub non_manifold_edge_count: usize,
    pub winding_mismatch_count: usize,
    pub signed_volume: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisBoundarySurface {
    pub part_id: String,
    pub label: String,
    pub source_geometry_digest: String,
    pub tessellation_policy: AnalysisBoundaryTessellationPolicy,
    pub vertices: Vec<[f64; 3]>,
    pub triangles: Vec<[u32; 3]>,
    pub triangle_face_group_indices: Vec<u32>,
    pub face_groups: Vec<AnalysisBoundaryFaceGroup>,
    pub evidence: AnalysisBoundaryEvidence,
    pub content_digest: String,
}

pub fn load_direct_occt_analysis_boundary_surface(
    bundle_dir: impl AsRef<Path>,
    part_id: &str,
) -> AppResult<AnalysisBoundarySurface> {
    let bundle_dir = bundle_dir.as_ref();
    let manifest = load_manifest(bundle_dir)?;
    let report = load_topology_report(bundle_dir)?;
    build_analysis_boundary_surface(&report, &manifest, part_id)
}

fn load_manifest(bundle_dir: &Path) -> AppResult<AnalysisBoundaryManifestView> {
    let manifest_path = bundle_dir.join("manifest.json");
    let manifest_text = fs::read_to_string(&manifest_path).map_err(|err| {
        AppError::persistence(format!(
            "Analysis boundary manifest '{}' could not be read: {}",
            manifest_path.display(),
            err
        ))
    })?;
    serde_json::from_str::<AnalysisBoundaryManifestView>(&manifest_text).map_err(|err| {
        AppError::validation(format!(
            "Analysis boundary manifest '{}' is invalid: {err}",
            manifest_path.display()
        ))
    })
}

fn load_topology_report(bundle_dir: &Path) -> AppResult<AnalysisBoundaryTopologyReport> {
    super::topology_artifact::read(&super::topology_artifact::analysis_boundary_path(
        bundle_dir,
    ))
}

fn build_analysis_boundary_surface(
    report: &AnalysisBoundaryTopologyReport,
    manifest: &AnalysisBoundaryManifestView,
    part_id: &str,
) -> AppResult<AnalysisBoundarySurface> {
    if report.schema_version != ANALYSIS_BOUNDARY_SCHEMA_VERSION {
        return Err(AppError::validation(format!(
            "Analysis boundary topology report schemaVersion {} is unsupported.",
            report.schema_version
        )));
    }
    let tessellation_policy = report.tessellation_policy;
    if !tessellation_policy.linear_deflection_mm.is_finite()
        || tessellation_policy.linear_deflection_mm <= 0.0
        || !tessellation_policy.angular_deflection_rad.is_finite()
        || tessellation_policy.angular_deflection_rad <= 0.0
        || tessellation_policy.angular_deflection_rad > std::f64::consts::PI
    {
        return Err(AppError::validation(format!(
            "Analysis boundary tessellation policy is invalid: linearDeflectionMm={}, angularDeflectionRad={}.",
            tessellation_policy.linear_deflection_mm,
            tessellation_policy.angular_deflection_rad
        )));
    }
    let part = report
        .parts
        .iter()
        .find(|candidate| candidate.part_id == part_id)
        .ok_or_else(|| {
            AppError::validation(format!(
                "Analysis boundary topology report does not contain partId '{}'.",
                part_id
            ))
        })?;
    if !matches!(
        part.representation.as_str(),
        "analyticBrep" | "analyticbrep"
    ) {
        return Err(AppError::validation(format!(
            "Analysis boundary surface requires an analytic BRep part, got representation '{}'.",
            part.representation
        )));
    }
    if part
        .source_geometry_digest
        .as_deref()
        .unwrap_or(" ")
        .trim()
        .is_empty()
    {
        return Err(AppError::validation(format!(
            "Analysis boundary part '{}' is missing sourceGeometryDigest.",
            part.part_id
        )));
    }
    if part.triangles.is_empty() {
        return Err(AppError::validation(format!(
            "Analysis boundary part '{}' contains no triangles.",
            part.part_id
        )));
    }
    if part.triangles.len() != part.triangle_face_group_indices.len() {
        return Err(AppError::validation(format!(
            "Analysis boundary part '{}' triangle face group cardinality mismatch: triangles={}, groups={}",
            part.part_id,
            part.triangles.len(),
            part.triangle_face_group_indices.len()
        )));
    }

    let mut face_groups = build_face_groups(part, manifest)?;
    let mut triangle_group_indices = Vec::with_capacity(part.triangles.len());
    for (triangle_index, (_triangle, group_index)) in part
        .triangles
        .iter()
        .zip(part.triangle_face_group_indices.iter())
        .enumerate()
    {
        let group_index = usize::try_from(*group_index).map_err(|_| {
            AppError::validation(format!(
                "Analysis boundary part '{}' triangle {} has out-of-range group index {}.",
                part.part_id, triangle_index, group_index
            ))
        })?;
        if group_index >= face_groups.len() {
            return Err(AppError::validation(format!(
                "Analysis boundary part '{}' triangle {} references face group {} but only {} groups exist.",
                part.part_id,
                triangle_index,
                group_index,
                face_groups.len()
            )));
        }
        triangle_group_indices.push(group_index as u32);
        let face_group_target_id = face_groups[group_index].target_id.clone();
        let triangle_count = face_groups[group_index].triangle_count;
        face_groups[group_index].triangle_count =
            triangle_count.checked_add(1).ok_or_else(|| {
                AppError::validation(format!(
                    "Analysis boundary part '{}' face group '{}' triangle count overflowed.",
                    part.part_id, face_group_target_id
                ))
            })?;
    }

    let (vertices, triangles) = normalize_boundary_geometry(&part.triangles)?;
    let evidence = validate_boundary_evidence(&vertices, &triangles)?;
    validate_brep_mesh_incidence(
        part,
        &vertices,
        &triangles,
        &triangle_group_indices,
        &face_groups,
    )?;
    validate_face_group_coverage(
        &part.part_id,
        &vertices,
        &triangles,
        &triangle_group_indices,
        &face_groups,
        tessellation_policy,
    )?;
    let content_digest = content_digest(
        &part.part_id,
        part.label.as_str(),
        part.source_geometry_digest.as_deref().unwrap_or_default(),
        tessellation_policy,
        &vertices,
        &triangles,
        &triangle_group_indices,
        &face_groups,
        &evidence,
    );

    Ok(AnalysisBoundarySurface {
        part_id: part.part_id.clone(),
        label: part.label.clone(),
        source_geometry_digest: part.source_geometry_digest.clone().unwrap_or_default(),
        tessellation_policy,
        vertices,
        triangles,
        triangle_face_group_indices: triangle_group_indices,
        face_groups,
        evidence,
        content_digest,
    })
}

fn build_face_groups(
    part: &AnalysisBoundaryTopologyPart,
    manifest: &AnalysisBoundaryManifestView,
) -> AppResult<Vec<AnalysisBoundaryFaceGroup>> {
    let mut face_groups = Vec::with_capacity(part.faces.len());
    let mut seen_targets = BTreeSet::new();
    for face in &part.faces {
        let target_id = face
            .target_id
            .as_deref()
            .map(str::trim)
            .filter(|target_id| !target_id.is_empty())
            .ok_or_else(|| {
                AppError::validation(format!(
                    "Analysis boundary part '{}' contains a face without targetId.",
                    part.part_id
                ))
            })?
            .to_string();
        if !seen_targets.insert(target_id.clone()) {
            return Err(AppError::validation(format!(
                "Analysis boundary part '{}' contains duplicate face targetId '{}'.",
                part.part_id, target_id
            )));
        }
        let Some(selection_target) = manifest
            .selection_targets
            .iter()
            .find(|target| selection_target_matches_id(target, &target_id))
        else {
            return Err(AppError::validation(format!(
                "Analysis boundary face targetId '{}' is missing from manifest selection targets.",
                target_id
            )));
        };
        let canonical_target_id = selection_target
            .canonical_target_id
            .clone()
            .unwrap_or_else(|| target_id.clone());
        let durable_target_id = selection_target.durable_target_id.clone();
        face_groups.push(AnalysisBoundaryFaceGroup {
            part_id: part.part_id.clone(),
            target_id,
            canonical_target_id,
            durable_target_id,
            label: face.label.clone(),
            area: face.area.unwrap_or_default(),
            triangle_count: 0,
        });
    }
    Ok(face_groups)
}

fn selection_target_matches_id(selection_target: &SelectionTarget, target_id: &str) -> bool {
    selection_target.target_id.as_deref() == Some(target_id)
        || selection_target.durable_target_id.as_deref() == Some(target_id)
        || selection_target.canonical_target_id.as_deref() == Some(target_id)
        || selection_target
            .alias_ids
            .iter()
            .any(|alias| alias == target_id)
}

fn validate_brep_mesh_incidence(
    part: &AnalysisBoundaryTopologyPart,
    mesh_vertices: &[[f64; 3]],
    mesh_triangles: &[[u32; 3]],
    triangle_group_indices: &[u32],
    face_groups: &[AnalysisBoundaryFaceGroup],
) -> AppResult<()> {
    let mut brep_vertices = BTreeMap::new();
    for vertex in &part.vertices {
        let target_id = required_topology_id(&part.part_id, "vertex", vertex.target_id.as_deref())?;
        let point = vertex.point.ok_or_else(|| {
            AppError::validation(format!(
                "Analysis boundary part '{}' BRep vertex '{}' is missing its exact point.",
                part.part_id, target_id
            ))
        })?;
        let point = [point.x, point.y, point.z];
        if !point.iter().all(|coordinate| coordinate.is_finite()) {
            return Err(AppError::validation(format!(
                "Analysis boundary part '{}' BRep vertex '{}' has non-finite coordinates.",
                part.part_id, target_id
            )));
        }
        if brep_vertices.insert(target_id.to_string(), point).is_some() {
            return Err(AppError::validation(format!(
                "Analysis boundary part '{}' contains duplicate BRep vertex targetId '{}'.",
                part.part_id, target_id
            )));
        }
    }
    for (target_id, point) in &brep_vertices {
        let represented = mesh_vertices.iter().any(|candidate| {
            candidate
                .iter()
                .zip(point)
                .map(|(left, right)| (left - right) * (left - right))
                .sum::<f64>()
                <= ANALYSIS_BOUNDARY_WELD_TOLERANCE_MM.powi(2)
        });
        if !represented {
            return Err(AppError::validation(format!(
                "Analysis boundary part '{}' BRep vertex '{}' is absent from the welded boundary mesh.",
                part.part_id, target_id
            )));
        }
    }

    let mut brep_edges = BTreeMap::<String, Vec<String>>::new();
    for edge in &part.edges {
        let target_id = required_topology_id(&part.part_id, "edge", edge.target_id.as_deref())?;
        let mut endpoint_ids = edge.vertex_target_ids.clone();
        if endpoint_ids.is_empty() {
            for endpoint in [edge.start, edge.end].into_iter().flatten() {
                let point = [endpoint.x, endpoint.y, endpoint.z];
                let Some((vertex_id, _)) = brep_vertices.iter().find(|(_, candidate)| {
                    candidate
                        .iter()
                        .zip(point)
                        .map(|(left, right)| (left - right) * (left - right))
                        .sum::<f64>()
                        <= ANALYSIS_BOUNDARY_WELD_TOLERANCE_MM.powi(2)
                }) else {
                    return Err(AppError::validation(format!(
                        "Analysis boundary part '{}' BRep edge '{}' endpoint is not an exact BRep vertex.",
                        part.part_id, target_id
                    )));
                };
                endpoint_ids.push(vertex_id.clone());
            }
        }
        if endpoint_ids.is_empty() || endpoint_ids.len() > 2 {
            return Err(AppError::validation(format!(
                "Analysis boundary part '{}' BRep edge '{}' must reference one or two endpoint vertices; observed {}.",
                part.part_id,
                target_id,
                endpoint_ids.len()
            )));
        }
        for vertex_id in &endpoint_ids {
            if !brep_vertices.contains_key(vertex_id.as_str()) {
                return Err(AppError::validation(format!(
                    "Analysis boundary part '{}' BRep edge '{}' references missing vertex '{}'.",
                    part.part_id, target_id, vertex_id
                )));
            }
        }
        if brep_edges
            .insert(target_id.to_string(), endpoint_ids)
            .is_some()
        {
            return Err(AppError::validation(format!(
                "Analysis boundary part '{}' contains duplicate BRep edge targetId '{}'.",
                part.part_id, target_id
            )));
        }
    }

    let mut edge_face_uses = BTreeMap::<String, Vec<usize>>::new();
    for (face_index, face) in part.faces.iter().enumerate() {
        let target_id = required_topology_id(&part.part_id, "face", face.target_id.as_deref())?;
        if target_id != face_groups[face_index].target_id {
            return Err(AppError::validation(format!(
                "Analysis boundary part '{}' face incidence order diverged at group {}: '{}' versus '{}'.",
                part.part_id, face_index, target_id, face_groups[face_index].target_id
            )));
        }
        for (loop_index, edge_ids) in face.boundary_loops().iter().enumerate() {
            if edge_ids.is_empty() {
                return Err(AppError::validation(format!(
                    "Analysis boundary part '{}' face '{}' has empty BRep loop {}.",
                    part.part_id, target_id, loop_index
                )));
            }
            for (index, edge_id) in edge_ids.iter().enumerate() {
                let endpoints = brep_edges.get(edge_id).ok_or_else(|| {
                    AppError::validation(format!(
                        "Analysis boundary part '{}' face '{}' loop {} references missing edge '{}'.",
                        part.part_id, target_id, loop_index, edge_id
                    ))
                })?;
                let next_id = &edge_ids[(index + 1) % edge_ids.len()];
                let next_endpoints = brep_edges.get(next_id).ok_or_else(|| {
                    AppError::validation(format!(
                        "Analysis boundary part '{}' face '{}' loop {} references missing edge '{}'.",
                        part.part_id, target_id, loop_index, next_id
                    ))
                })?;
                if !endpoints
                    .iter()
                    .any(|endpoint| next_endpoints.contains(endpoint))
                {
                    return Err(AppError::validation(format!(
                        "Analysis boundary part '{}' face '{}' BRep loop {} is disconnected between '{}' and '{}'.",
                        part.part_id, target_id, loop_index, edge_id, next_id
                    )));
                }
                edge_face_uses
                    .entry(edge_id.clone())
                    .or_default()
                    .push(face_index);
            }
        }
    }
    for edge_id in brep_edges.keys() {
        let uses = edge_face_uses.get(edge_id).map(Vec::len).unwrap_or(0);
        if uses != 2 {
            return Err(AppError::validation(format!(
                "Analysis boundary part '{}' BRep edge '{}' has {} face-loop incidences; closed solid requires 2.",
                part.part_id, edge_id, uses
            )));
        }
    }

    let brep_adjacency = edge_face_uses
        .values()
        .filter_map(|uses| {
            let mut unique = uses.clone();
            unique.sort_unstable();
            unique.dedup();
            (unique.len() == 2).then(|| (unique[0], unique[1]))
        })
        .collect::<BTreeSet<_>>();
    let mut mesh_edge_groups = BTreeMap::<(u32, u32), Vec<usize>>::new();
    for (triangle, group) in mesh_triangles.iter().zip(triangle_group_indices) {
        let group = *group as usize;
        for (from, to) in [
            (triangle[0], triangle[1]),
            (triangle[1], triangle[2]),
            (triangle[2], triangle[0]),
        ] {
            mesh_edge_groups
                .entry((from.min(to), from.max(to)))
                .or_default()
                .push(group);
        }
    }
    let mesh_adjacency = mesh_edge_groups
        .values()
        .filter_map(|groups| {
            let mut unique = groups.clone();
            unique.sort_unstable();
            unique.dedup();
            (unique.len() == 2).then(|| (unique[0], unique[1]))
        })
        .collect::<BTreeSet<_>>();
    if brep_adjacency != mesh_adjacency {
        return Err(AppError::validation(format!(
            "Analysis boundary part '{}' BRep/mesh face-edge incidence mismatch: BRep {:?}, mesh {:?}.",
            part.part_id, brep_adjacency, mesh_adjacency
        )));
    }
    Ok(())
}

fn required_topology_id<'a>(
    part_id: &str,
    kind: &str,
    value: Option<&'a str>,
) -> AppResult<&'a str> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AppError::validation(format!(
                "Analysis boundary part '{}' contains a BRep {} without targetId.",
                part_id, kind
            ))
        })
}

fn validate_face_group_coverage(
    part_id: &str,
    vertices: &[[f64; 3]],
    triangles: &[[u32; 3]],
    triangle_group_indices: &[u32],
    face_groups: &[AnalysisBoundaryFaceGroup],
    tessellation_policy: AnalysisBoundaryTessellationPolicy,
) -> AppResult<()> {
    let mut observed_area = vec![0.0; face_groups.len()];
    for (triangle, group_index) in triangles.iter().zip(triangle_group_indices) {
        let a = vertices[triangle[0] as usize];
        let b = vertices[triangle[1] as usize];
        let c = vertices[triangle[2] as usize];
        observed_area[*group_index as usize] += triangle_area(a, b, c);
    }
    for (index, group) in face_groups.iter().enumerate() {
        let expected = group.area;
        let observed = observed_area[index];
        let chord_error = tessellation_policy.linear_deflection_mm;
        let chord_area_tolerance = chord_error.mul_add(expected.abs().sqrt(), chord_error.powi(2));
        let tolerance = (expected.abs() * ANALYSIS_BOUNDARY_FACE_AREA_RELATIVE_TOLERANCE)
            .max(chord_area_tolerance)
            .max(1.0e-6);
        if !expected.is_finite()
            || expected <= 0.0
            || !observed.is_finite()
            || observed <= 0.0
            || (expected - observed).abs() > tolerance
        {
            return Err(AppError::validation(format!(
                "Analysis boundary face coverage failed: partId='{}', selector='{}', canonicalTargetId='{}', durableTargetId='{}', expectedAreaMm2={}, observedAreaMm2={}, toleranceMm2={}.",
                part_id,
                group.target_id,
                group.canonical_target_id,
                group.durable_target_id.as_deref().unwrap_or(""),
                expected,
                observed,
                tolerance
            )));
        }
    }
    Ok(())
}

fn triangle_area(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let cross = [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ];
    0.5 * cross
        .into_iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt()
}

fn normalize_boundary_geometry(
    raw_triangles: &[AnalysisBoundaryTriangle],
) -> AppResult<(Vec<[f64; 3]>, Vec<[u32; 3]>)> {
    let mut vertices = Vec::new();
    let mut vertex_cells: BTreeMap<[i64; 3], Vec<u32>> = BTreeMap::new();
    let mut triangles = Vec::with_capacity(raw_triangles.len());
    let mut seen_canonical_triangles = BTreeSet::new();

    for (triangle_index, triangle) in raw_triangles.iter().enumerate() {
        let mut indexed = [0_u32; 3];
        for (corner, vertex) in triangle.vertices.iter().enumerate() {
            indexed[corner] =
                weld_vertex(&mut vertices, &mut vertex_cells, *vertex).map_err(|err| {
                    AppError::validation(format!(
                        "Analysis boundary triangle {} has invalid vertex {}: {}",
                        triangle_index, corner, err
                    ))
                })?;
        }
        if indexed[0] == indexed[1] || indexed[1] == indexed[2] || indexed[2] == indexed[0] {
            return Err(AppError::validation(format!(
                "Analysis boundary triangle {} is degenerate (repeated vertex index).",
                triangle_index
            )));
        }
        let a = vertices[indexed[0] as usize];
        let b = vertices[indexed[1] as usize];
        let c = vertices[indexed[2] as usize];
        if triangle_is_degenerate(a, b, c) {
            return Err(AppError::validation(format!(
                "Analysis boundary triangle {} is degenerate (zero area).",
                triangle_index
            )));
        }
        let mut canonical = indexed;
        canonical.sort_unstable();
        if !seen_canonical_triangles.insert(canonical) {
            return Err(AppError::validation(format!(
                "Analysis boundary triangle {} duplicates a previously emitted triangle.",
                triangle_index
            )));
        }
        triangles.push(indexed);
    }

    let signed_volume = signed_volume(&vertices, &triangles);
    if signed_volume < 0.0 {
        for triangle in &mut triangles {
            triangle.swap(1, 2);
        }
    }
    Ok((vertices, triangles))
}

fn validate_boundary_evidence(
    vertices: &[[f64; 3]],
    triangles: &[[u32; 3]],
) -> AppResult<AnalysisBoundaryEvidence> {
    let mut edges: BTreeMap<(u32, u32), Vec<(usize, bool)>> = BTreeMap::new();
    let mut neighbours = vec![Vec::new(); triangles.len()];
    for (face_index, triangle) in triangles.iter().enumerate() {
        for index in triangle {
            if (*index as usize) >= vertices.len() {
                return Err(AppError::validation(format!(
                    "Analysis boundary triangle {} references an out-of-bounds vertex index {}.",
                    face_index, index
                )));
            }
        }
        let a = vertices[triangle[0] as usize];
        let b = vertices[triangle[1] as usize];
        let c = vertices[triangle[2] as usize];
        if triangle_is_degenerate(a, b, c) {
            return Err(AppError::validation(format!(
                "Analysis boundary triangle {} is degenerate.",
                face_index
            )));
        }
        for (from, to) in [
            (triangle[0], triangle[1]),
            (triangle[1], triangle[2]),
            (triangle[2], triangle[0]),
        ] {
            let key = (from.min(to), from.max(to));
            edges
                .entry(key)
                .or_default()
                .push((face_index, from == key.0));
        }
    }

    let mut boundary_edge_count = 0;
    let mut non_manifold_edge_count = 0;
    let mut winding_mismatch_count = 0;
    for adjacent_faces in edges.values() {
        match adjacent_faces.as_slice() {
            [_] => boundary_edge_count += 1,
            [(_, first_forward), (_, second_forward)] => {
                if first_forward == second_forward {
                    winding_mismatch_count += 1;
                }
            }
            _ => non_manifold_edge_count += 1,
        }
        for left in 0..adjacent_faces.len() {
            for right in (left + 1)..adjacent_faces.len() {
                let first_face = adjacent_faces[left].0;
                let second_face = adjacent_faces[right].0;
                neighbours[first_face].push(second_face);
                neighbours[second_face].push(first_face);
            }
        }
    }

    let component_count = connected_component_count(&neighbours);
    let signed_volume = signed_volume(vertices, triangles);
    let closed = !triangles.is_empty()
        && boundary_edge_count == 0
        && non_manifold_edge_count == 0
        && winding_mismatch_count == 0;
    let manifold =
        boundary_edge_count == 0 && non_manifold_edge_count == 0 && winding_mismatch_count == 0;
    let positive_volume = signed_volume > 0.0;

    if !closed {
        return Err(AppError::validation(format!(
            "Analysis boundary surface is open: boundary edges: {}; non-manifold edges: {}; winding mismatches: {}.",
            boundary_edge_count, non_manifold_edge_count, winding_mismatch_count
        )));
    }
    if non_manifold_edge_count > 0 {
        return Err(AppError::validation(format!(
            "Analysis boundary surface is non-manifold: {} non-manifold edges.",
            non_manifold_edge_count
        )));
    }
    if component_count != 1 {
        return Err(AppError::validation(format!(
            "Analysis boundary surface must contain exactly one connected component, got {}.",
            component_count
        )));
    }
    if !positive_volume {
        return Err(AppError::validation(format!(
            "Analysis boundary surface must enclose positive volume; signed volume = {}.",
            signed_volume
        )));
    }

    Ok(AnalysisBoundaryEvidence {
        closed,
        manifold,
        component_count,
        positive_volume,
        boundary_edge_count,
        non_manifold_edge_count,
        winding_mismatch_count,
        signed_volume,
    })
}

fn signed_volume(vertices: &[[f64; 3]], triangles: &[[u32; 3]]) -> f64 {
    let mut volume = 0.0;
    for triangle in triangles {
        let a = vertices[triangle[0] as usize];
        let b = vertices[triangle[1] as usize];
        let c = vertices[triangle[2] as usize];
        volume += tetrahedron_signed_volume([0.0, 0.0, 0.0], a, b, c);
    }
    volume
}

fn tetrahedron_signed_volume(a: [f64; 3], b: [f64; 3], c: [f64; 3], d: [f64; 3]) -> f64 {
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let ad = [d[0] - a[0], d[1] - a[1], d[2] - a[2]];
    (ab[0] * (ac[1] * ad[2] - ac[2] * ad[1]) - ab[1] * (ac[0] * ad[2] - ac[2] * ad[0])
        + ab[2] * (ac[0] * ad[1] - ac[1] * ad[0]))
        / 6.0
}

fn connected_component_count(neighbours: &[Vec<usize>]) -> usize {
    let mut seen = vec![false; neighbours.len()];
    let mut count = 0;
    for start in 0..neighbours.len() {
        if seen[start] {
            continue;
        }
        count += 1;
        seen[start] = true;
        let mut stack = vec![start];
        while let Some(face) = stack.pop() {
            for &neighbour in &neighbours[face] {
                if !seen[neighbour] {
                    seen[neighbour] = true;
                    stack.push(neighbour);
                }
            }
        }
    }
    count
}

fn weld_vertex(
    vertices: &mut Vec<[f64; 3]>,
    vertex_cells: &mut BTreeMap<[i64; 3], Vec<u32>>,
    position: [f64; 3],
) -> Result<u32, &'static str> {
    if !position.iter().all(|value| value.is_finite()) {
        return Err("non-finite coordinate");
    }
    let position = canonicalize_zero_vector(position);
    let cell = position
        .map(|coordinate| (coordinate / ANALYSIS_BOUNDARY_WELD_TOLERANCE_MM).floor() as i64);
    for dx in -1..=1 {
        for dy in -1..=1 {
            for dz in -1..=1 {
                let neighbour = [cell[0] + dx, cell[1] + dy, cell[2] + dz];
                for index in vertex_cells.get(&neighbour).into_iter().flatten() {
                    let existing = vertices[*index as usize];
                    let distance_squared = existing
                        .iter()
                        .zip(position)
                        .map(|(left, right)| {
                            let delta = left - right;
                            delta * delta
                        })
                        .sum::<f64>();
                    if distance_squared
                        <= ANALYSIS_BOUNDARY_WELD_TOLERANCE_MM * ANALYSIS_BOUNDARY_WELD_TOLERANCE_MM
                    {
                        return Ok(*index);
                    }
                }
            }
        }
    }
    let index = u32::try_from(vertices.len()).map_err(|_| "too many vertices")?;
    vertices.push(position);
    vertex_cells.entry(cell).or_default().push(index);
    Ok(index)
}

fn canonicalize_zero_vector(position: [f64; 3]) -> [f64; 3] {
    position.map(|value| if value == 0.0 { 0.0 } else { value })
}

fn triangle_is_degenerate(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> bool {
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let scale = ab
        .into_iter()
        .chain(ac)
        .map(f64::abs)
        .fold(0.0_f64, f64::max);
    if scale == 0.0 {
        return true;
    }
    let ab = ab.map(|value| value / scale);
    let ac = ac.map(|value| value / scale);
    let cross = [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ];
    let normalized_double_area_squared =
        cross[0].mul_add(cross[0], cross[1].mul_add(cross[1], cross[2] * cross[2]));
    normalized_double_area_squared <= f64::EPSILON
}

fn content_digest(
    part_id: &str,
    label: &str,
    source_geometry_digest: &str,
    tessellation_policy: AnalysisBoundaryTessellationPolicy,
    vertices: &[[f64; 3]],
    triangles: &[[u32; 3]],
    triangle_group_indices: &[u32],
    face_groups: &[AnalysisBoundaryFaceGroup],
    evidence: &AnalysisBoundaryEvidence,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"ecky-analysis-boundary-v2\0");
    hasher.update(part_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(label.as_bytes());
    hasher.update(b"\0");
    hasher.update(source_geometry_digest.as_bytes());
    hasher.update(b"\0");
    hasher.update(canonical_f64(tessellation_policy.linear_deflection_mm).as_bytes());
    hasher.update(b"\0");
    hasher.update(canonical_f64(tessellation_policy.angular_deflection_rad).as_bytes());
    hasher.update(b"\0");
    hasher.update((vertices.len() as u64).to_le_bytes());
    for vertex in vertices {
        for coordinate in vertex {
            hasher.update(canonical_f64(*coordinate).as_bytes());
            hasher.update(b"\0");
        }
    }
    hasher.update((triangles.len() as u64).to_le_bytes());
    for triangle in triangles {
        for index in triangle {
            hasher.update(index.to_le_bytes());
        }
    }
    hasher.update((triangle_group_indices.len() as u64).to_le_bytes());
    for group_index in triangle_group_indices {
        hasher.update(group_index.to_le_bytes());
    }
    hasher.update((face_groups.len() as u64).to_le_bytes());
    for face_group in face_groups {
        hasher.update(face_group.part_id.as_bytes());
        hasher.update(b"\0");
        hasher.update(face_group.target_id.as_bytes());
        hasher.update(b"\0");
        hasher.update(face_group.canonical_target_id.as_bytes());
        hasher.update(b"\0");
        if let Some(durable_target_id) = face_group.durable_target_id.as_deref() {
            hasher.update(durable_target_id.as_bytes());
        }
        hasher.update(b"\0");
        hasher.update(face_group.label.as_bytes());
        hasher.update(b"\0");
        hasher.update(canonical_f64(face_group.area).as_bytes());
        hasher.update(b"\0");
        hasher.update(face_group.triangle_count.to_le_bytes());
    }
    hasher.update((evidence.closed as u8).to_le_bytes());
    hasher.update((evidence.manifold as u8).to_le_bytes());
    hasher.update((evidence.component_count as u64).to_le_bytes());
    hasher.update((evidence.positive_volume as u8).to_le_bytes());
    hasher.update((evidence.boundary_edge_count as u64).to_le_bytes());
    hasher.update((evidence.non_manifold_edge_count as u64).to_le_bytes());
    hasher.update((evidence.winding_mismatch_count as u64).to_le_bytes());
    hasher.update(canonical_f64(evidence.signed_volume).as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

fn canonical_f64(value: f64) -> String {
    if value == 0.0 {
        return "f64:0000000000000000".to_string();
    }
    let raw = value.to_bits();
    format!("f64:{raw:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::DesignParams;
    use crate::ecky_cad_host::direct_occt_runtime::render_core_program_runtime_bundle;
    use crate::ecky_cad_host::direct_occt_sdk::{
        bundled_occt_runtime_root_from_repo, inspect_occt_runtime,
    };
    use crate::models::PathResolver;
    use std::path::PathBuf;

    #[derive(Clone)]
    struct TestResolver {
        root: PathBuf,
    }

    impl PathResolver for TestResolver {
        fn app_config_dir(&self) -> PathBuf {
            self.root.join("config")
        }

        fn app_data_dir(&self) -> PathBuf {
            self.root.join("data")
        }

        fn resource_path(&self, _path: &str) -> Option<PathBuf> {
            None
        }
    }

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ecky-analysis-boundary-{}-{}",
            label,
            uuid::Uuid::new_v4()
        ))
    }

    fn manifest_view_with_targets() -> AnalysisBoundaryManifestView {
        AnalysisBoundaryManifestView {
            selection_targets: vec![
                SelectionTarget {
                    target_id: Some("body:face:0".to_string()),
                    durable_target_id: Some("body:node:7:face:0".to_string()),
                    canonical_target_id: Some("body:face:0".to_string()),
                    alias_ids: vec![],
                    part_id: "body".to_string(),
                    viewer_node_id: "body-viewer".to_string(),
                    label: "Body.Face1".to_string(),
                    kind: crate::contracts::SelectionTargetKind::Face,
                    editable: false,
                    parameter_keys: vec![],
                    primitive_ids: vec![],
                    view_ids: vec![],
                },
                SelectionTarget {
                    target_id: Some("body:face:1".to_string()),
                    durable_target_id: Some("body:node:7:face:1".to_string()),
                    canonical_target_id: Some("body:face:1".to_string()),
                    alias_ids: vec![],
                    part_id: "body".to_string(),
                    viewer_node_id: "body-viewer".to_string(),
                    label: "Body.Face2".to_string(),
                    kind: crate::contracts::SelectionTargetKind::Face,
                    editable: false,
                    parameter_keys: vec![],
                    primitive_ids: vec![],
                    view_ids: vec![],
                },
                SelectionTarget {
                    target_id: Some("body:face:2".to_string()),
                    durable_target_id: Some("body:node:7:face:2".to_string()),
                    canonical_target_id: Some("body:face:2".to_string()),
                    alias_ids: vec![],
                    part_id: "body".to_string(),
                    viewer_node_id: "body-viewer".to_string(),
                    label: "Body.Face3".to_string(),
                    kind: crate::contracts::SelectionTargetKind::Face,
                    editable: false,
                    parameter_keys: vec![],
                    primitive_ids: vec![],
                    view_ids: vec![],
                },
                SelectionTarget {
                    target_id: Some("body:face:3".to_string()),
                    durable_target_id: Some("body:node:7:face:3".to_string()),
                    canonical_target_id: Some("body:face:3".to_string()),
                    alias_ids: vec![],
                    part_id: "body".to_string(),
                    viewer_node_id: "body-viewer".to_string(),
                    label: "Body.Face4".to_string(),
                    kind: crate::contracts::SelectionTargetKind::Face,
                    editable: false,
                    parameter_keys: vec![],
                    primitive_ids: vec![],
                    view_ids: vec![],
                },
            ],
        }
    }

    fn valid_tetrahedron_report() -> AnalysisBoundaryTopologyReport {
        let point = |x, y, z| AnalysisBoundaryPoint { x, y, z };
        let vertex = |id: &str, point| AnalysisBoundaryTopologyVertex {
            target_id: Some(id.to_string()),
            point: Some(point),
        };
        let edge = |id: &str, start: &str, end: &str| AnalysisBoundaryTopologyEdge {
            target_id: Some(id.to_string()),
            vertex_target_ids: vec![start.to_string(), end.to_string()],
            start: None,
            end: None,
        };
        let face = |index: u32, area: f64, loop_ids: &[&str]| AnalysisBoundaryTopologyFace {
            target_id: Some(format!("body:face:{index}")),
            face_index: Some(index),
            label: format!("Body.Face{}", index + 1),
            center: None,
            normal: None,
            area: Some(area),
            boundary_edge_target_ids: vec![loop_ids.iter().map(|id| id.to_string()).collect()],
            exact_geometry: None,
        };
        AnalysisBoundaryTopologyReport {
            schema_version: ANALYSIS_BOUNDARY_SCHEMA_VERSION,
            tessellation_policy: AnalysisBoundaryTessellationPolicy::default(),
            parts: vec![AnalysisBoundaryTopologyPart {
                part_id: "body".to_string(),
                label: "Body".to_string(),
                representation: "analyticBrep".to_string(),
                source_geometry_digest: Some("sha256:source".to_string()),
                triangles: vec![
                    AnalysisBoundaryTriangle {
                        vertices: [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]],
                    },
                    AnalysisBoundaryTriangle {
                        vertices: [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
                    },
                    AnalysisBoundaryTriangle {
                        vertices: [[0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]],
                    },
                    AnalysisBoundaryTriangle {
                        vertices: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                    },
                ],
                triangle_face_group_indices: vec![0, 1, 2, 3],
                vertices: vec![
                    vertex("v0", point(0.0, 0.0, 0.0)),
                    vertex("v1", point(1.0, 0.0, 0.0)),
                    vertex("v2", point(0.0, 1.0, 0.0)),
                    vertex("v3", point(0.0, 0.0, 1.0)),
                ],
                edges: vec![
                    edge("e01", "v0", "v1"),
                    edge("e02", "v0", "v2"),
                    edge("e03", "v0", "v3"),
                    edge("e12", "v1", "v2"),
                    edge("e13", "v1", "v3"),
                    edge("e23", "v2", "v3"),
                ],
                faces: vec![
                    face(0, 0.5, &["e02", "e12", "e01"]),
                    face(1, 0.5, &["e01", "e13", "e03"]),
                    face(2, 0.5, &["e03", "e23", "e02"]),
                    face(3, 3.0_f64.sqrt() / 2.0, &["e12", "e23", "e13"]),
                ],
            }],
        }
    }

    #[test]
    fn exact_brep_and_boundary_mesh_incidence_must_match() {
        let report = valid_tetrahedron_report();
        let surface =
            build_analysis_boundary_surface(&report, &manifest_view_with_targets(), "body")
                .expect("matching tetrahedron incidence");
        assert_eq!(surface.face_groups.len(), 4);

        let mut mismatched = report;
        mismatched.parts[0].triangle_face_group_indices = vec![0, 0, 2, 3];
        let error =
            build_analysis_boundary_surface(&mismatched, &manifest_view_with_targets(), "body")
                .expect_err("mismatched mesh group adjacency");
        assert!(error
            .to_string()
            .contains("BRep/mesh face-edge incidence mismatch"));
    }

    #[test]
    fn loader_prefers_dedicated_analysis_boundary_over_viewer_topology() {
        let root = temp_root("dedicated-boundary");
        fs::create_dir_all(&root).expect("bundle directory");
        fs::write(
            root.join("manifest.json"),
            serde_json::to_vec(&manifest_view_with_targets()).expect("manifest json"),
        )
        .expect("write manifest");

        let analysis_boundary = valid_tetrahedron_report();
        let mut viewer_topology = analysis_boundary.clone();
        viewer_topology.parts[0].triangles.clear();
        viewer_topology.parts[0].triangle_face_group_indices.clear();
        fs::write(
            root.join("topology.json"),
            serde_json::to_vec(&viewer_topology).expect("viewer topology json"),
        )
        .expect("write viewer topology");
        fs::write(
            root.join("analysis-boundary.json"),
            serde_json::to_vec(&analysis_boundary).expect("analysis boundary json"),
        )
        .expect("write analysis boundary");

        let surface = load_direct_occt_analysis_boundary_surface(&root, "body")
            .expect("dedicated analysis boundary");
        assert_eq!(surface.triangles.len(), 4);
        assert!(surface.evidence.closed);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn exact_face_area_must_match_grouped_boundary_facets() {
        let mut report = valid_tetrahedron_report();
        report.parts[0].faces[0].area = Some(0.75);
        let error = build_analysis_boundary_surface(&report, &manifest_view_with_targets(), "body")
            .expect_err("partial face coverage");
        let detail = error.to_string();
        assert!(detail.contains("partId='body'"));
        assert!(detail.contains("selector='body:face:0'"));
        assert!(detail.contains("expectedAreaMm2=0.75"));
        assert!(detail.contains("observedAreaMm2=0.5"));
    }

    #[test]
    fn face_coverage_tolerance_accounts_for_declared_chord_error() {
        let mut report = valid_tetrahedron_report();
        report.tessellation_policy = AnalysisBoundaryTessellationPolicy {
            linear_deflection_mm: 0.25,
            angular_deflection_rad: 0.30,
        };
        report.parts[0].faces[0].area = Some(0.609);

        let surface =
            build_analysis_boundary_surface(&report, &manifest_view_with_targets(), "body")
                .expect("area deviation inside declared tessellation policy");
        assert_eq!(surface.face_groups[0].triangle_count, 1);
    }

    #[test]
    fn face_coverage_accepts_small_faceting_error_on_a_small_exact_face() {
        validate_face_group_coverage(
            "body",
            &[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            &[[0, 1, 2]],
            &[0],
            &[AnalysisBoundaryFaceGroup {
                part_id: "body".into(),
                target_id: "body:face:small".into(),
                canonical_target_id: "body:face:small".into(),
                durable_target_id: None,
                label: "small exact face".into(),
                area: 5.127,
                triangle_count: 1,
            }],
            AnalysisBoundaryTessellationPolicy::default(),
        )
        .expect("2.5 percent small-face faceting error");
    }

    #[test]
    fn integration_loads_closed_solid_and_preserves_provenance() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .to_path_buf();
        let runtime_root = bundled_occt_runtime_root_from_repo(&repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_occt_runtime(&runtime_root);
        if !layout.runtime_complete() {
            return;
        }
        let root = temp_root("integration-closed-solid");
        let resolver = TestResolver { root: root.clone() };
        let source = "(model (part body (box 20 20 10)))";
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("compile");
        let params = DesignParams::new();
        let (bundle, _manifest) =
            render_core_program_runtime_bundle(&program, source, &params, &layout, &resolver)
                .expect("direct occt render");
        let bundle_dir = crate::model_runtime::runtime_bundle_dir(&resolver, &bundle.model_id)
            .expect("bundle dir");
        let preview_digest_before = digest_file(&bundle_dir.join("model.stl"));
        let step_digest_before = digest_file(&bundle_dir.join("model.step"));

        let surface = load_direct_occt_analysis_boundary_surface(&bundle_dir, "body")
            .expect("analysis boundary surface");
        assert_eq!(surface.part_id, "body");
        assert!(surface.source_geometry_digest.starts_with("sha256:"));
        assert!(!surface.vertices.is_empty());
        assert!(!surface.triangles.is_empty());
        assert_eq!(
            surface.triangles.len(),
            surface.triangle_face_group_indices.len()
        );
        assert_eq!(surface.evidence.closed, true);
        assert_eq!(surface.evidence.manifold, true);
        assert_eq!(surface.evidence.component_count, 1);
        assert_eq!(surface.evidence.positive_volume, true);
        assert!(surface.evidence.signed_volume > 0.0);
        assert!(surface
            .face_groups
            .iter()
            .all(|group| group.canonical_target_id.starts_with("body:face:")));
        assert!(surface.face_groups.iter().all(|group| group
            .durable_target_id
            .as_deref()
            .unwrap_or("")
            .starts_with("body:stable-node-key:")));
        assert_eq!(
            surface.content_digest,
            load_direct_occt_analysis_boundary_surface(&bundle_dir, "body")
                .expect("repeat load")
                .content_digest
        );

        let preview_digest_after = digest_file(&bundle_dir.join("model.stl"));
        let step_digest_after = digest_file(&bundle_dir.join("model.step"));
        assert_eq!(preview_digest_before, preview_digest_after);
        assert_eq!(step_digest_before, step_digest_after);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn integration_emits_coarser_dedicated_boundary_without_mutating_exports() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .to_path_buf();
        let runtime_root = bundled_occt_runtime_root_from_repo(&repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_occt_runtime(&runtime_root);
        if !layout.runtime_complete() {
            return;
        }
        let root = temp_root("integration-dedicated-boundary");
        let resolver = TestResolver { root: root.clone() };
        let source = "(model (part body (cylinder 20 80)))";
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("compile");
        let params = DesignParams::new();
        let (bundle, _manifest) =
            render_core_program_runtime_bundle(&program, source, &params, &layout, &resolver)
                .expect("direct occt render");
        let bundle_dir = crate::model_runtime::runtime_bundle_dir(&resolver, &bundle.model_id)
            .expect("bundle dir");

        let viewer: AnalysisBoundaryTopologyReport = super::super::topology_artifact::read(
            &super::super::topology_artifact::topology_path(&bundle_dir),
        )
        .expect("viewer topology");
        let analysis = load_topology_report(&bundle_dir).expect("dedicated analysis boundary");
        assert_eq!(viewer.parts.len(), 1);
        assert_eq!(analysis.parts.len(), 1);
        assert_eq!(
            viewer.parts[0].source_geometry_digest, analysis.parts[0].source_geometry_digest,
            "analysis tessellation must not redefine source BRep identity"
        );
        assert!(
            analysis.parts[0].triangles.len() < viewer.parts[0].triangles.len(),
            "FEM boundary must be coarser than viewer/export tessellation"
        );

        let surface = load_direct_occt_analysis_boundary_surface(&bundle_dir, "body")
            .expect("load dedicated boundary");
        assert_eq!(surface.triangles.len(), analysis.parts[0].triangles.len());
        assert!(surface.evidence.closed);
        assert!(surface.evidence.manifold);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_group_cardinality_mismatch() {
        let report = AnalysisBoundaryTopologyReport {
            schema_version: ANALYSIS_BOUNDARY_SCHEMA_VERSION,
            tessellation_policy: AnalysisBoundaryTessellationPolicy::default(),
            parts: vec![AnalysisBoundaryTopologyPart {
                part_id: "body".to_string(),
                label: "Body".to_string(),
                representation: "analyticBrep".to_string(),
                source_geometry_digest: Some("sha256:source".to_string()),
                triangles: vec![
                    AnalysisBoundaryTriangle {
                        vertices: [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                    },
                    AnalysisBoundaryTriangle {
                        vertices: [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                    },
                ],
                triangle_face_group_indices: vec![0],
                vertices: vec![],
                edges: vec![],
                faces: vec![AnalysisBoundaryTopologyFace {
                    target_id: Some("body:face:0".to_string()),
                    face_index: Some(0),
                    label: "Body.Face1".to_string(),
                    center: None,
                    normal: None,
                    area: Some(0.5),
                    boundary_edge_target_ids: vec![],
                    exact_geometry: None,
                }],
            }],
        };
        let err = build_analysis_boundary_surface(&report, &manifest_view_with_targets(), "body")
            .expect_err("group cardinality mismatch");
        assert!(err.to_string().contains("triangle face group cardinality"));
    }

    #[test]
    fn rejects_open_boundary() {
        let report = AnalysisBoundaryTopologyReport {
            schema_version: ANALYSIS_BOUNDARY_SCHEMA_VERSION,
            tessellation_policy: AnalysisBoundaryTessellationPolicy::default(),
            parts: vec![AnalysisBoundaryTopologyPart {
                part_id: "body".to_string(),
                label: "Body".to_string(),
                representation: "analyticBrep".to_string(),
                source_geometry_digest: Some("sha256:source".to_string()),
                triangles: vec![AnalysisBoundaryTriangle {
                    vertices: [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                }],
                triangle_face_group_indices: vec![0],
                vertices: vec![],
                edges: vec![],
                faces: vec![AnalysisBoundaryTopologyFace {
                    target_id: Some("body:face:0".to_string()),
                    face_index: Some(0),
                    label: "Body.Face1".to_string(),
                    center: None,
                    normal: None,
                    area: Some(0.5),
                    boundary_edge_target_ids: vec![],
                    exact_geometry: None,
                }],
            }],
        };
        let err = build_analysis_boundary_surface(&report, &manifest_view_with_targets(), "body")
            .expect_err("open boundary");
        assert!(err.to_string().contains("open"));
    }

    #[test]
    fn rejects_non_manifold_boundary() {
        let report = AnalysisBoundaryTopologyReport {
            schema_version: ANALYSIS_BOUNDARY_SCHEMA_VERSION,
            tessellation_policy: AnalysisBoundaryTessellationPolicy::default(),
            parts: vec![AnalysisBoundaryTopologyPart {
                part_id: "body".to_string(),
                label: "Body".to_string(),
                representation: "analyticBrep".to_string(),
                source_geometry_digest: Some("sha256:source".to_string()),
                triangles: vec![
                    AnalysisBoundaryTriangle {
                        vertices: [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                    },
                    AnalysisBoundaryTriangle {
                        vertices: [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
                    },
                    AnalysisBoundaryTriangle {
                        vertices: [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]],
                    },
                ],
                triangle_face_group_indices: vec![0, 0, 0],
                vertices: vec![],
                edges: vec![],
                faces: vec![AnalysisBoundaryTopologyFace {
                    target_id: Some("body:face:0".to_string()),
                    face_index: Some(0),
                    label: "Body.Face1".to_string(),
                    center: None,
                    normal: None,
                    area: Some(1.5),
                    boundary_edge_target_ids: vec![],
                    exact_geometry: None,
                }],
            }],
        };
        let err = build_analysis_boundary_surface(&report, &manifest_view_with_targets(), "body")
            .expect_err("non-manifold boundary");
        assert!(err.to_string().contains("non-manifold"));
    }

    #[test]
    fn rejects_degenerate_boundary() {
        let report = AnalysisBoundaryTopologyReport {
            schema_version: ANALYSIS_BOUNDARY_SCHEMA_VERSION,
            tessellation_policy: AnalysisBoundaryTessellationPolicy::default(),
            parts: vec![AnalysisBoundaryTopologyPart {
                part_id: "body".to_string(),
                label: "Body".to_string(),
                representation: "analyticBrep".to_string(),
                source_geometry_digest: Some("sha256:source".to_string()),
                triangles: vec![AnalysisBoundaryTriangle {
                    vertices: [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
                }],
                triangle_face_group_indices: vec![0],
                vertices: vec![],
                edges: vec![],
                faces: vec![AnalysisBoundaryTopologyFace {
                    target_id: Some("body:face:0".to_string()),
                    face_index: Some(0),
                    label: "Body.Face1".to_string(),
                    center: None,
                    normal: None,
                    area: Some(0.0),
                    boundary_edge_target_ids: vec![],
                    exact_geometry: None,
                }],
            }],
        };
        let err = build_analysis_boundary_surface(&report, &manifest_view_with_targets(), "body")
            .expect_err("degenerate boundary");
        assert!(err.to_string().contains("degenerate"));
    }

    fn digest_file(path: &Path) -> String {
        let bytes = fs::read(path).expect("read file");
        format!("sha256:{:x}", Sha256::digest(bytes))
    }
}
