use std::collections::{BTreeMap, HashMap};

use csgrs::float_types::parry3d::na::{Point3, Vector3};
use csgrs::mesh::polygon::Polygon as IrPolygon;
use csgrs::mesh::vertex::Vertex as IrVertex;

use crate::models::{AppResult, ParamValue};

use super::eval_scalar::eval_stringish;
use super::model::{expr_keyword_name, IrExpr};
use super::shared::{validation, IrMesh};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EdgeSelector {
    All,
    Top,
    Bottom,
    Vertical,
}

/// A feature edge between two triangles, identified by canonical vertex indices.
#[derive(Debug, Clone)]
pub(super) struct FeatureEdge {
    /// Canonical vertex indices (a < b).
    vi_a: usize,
    vi_b: usize,
    /// Positions of the two endpoints.
    pos_a: Point3<f64>,
    pos_b: Point3<f64>,
    /// Normals of the two adjacent faces.
    normal_left: Vector3<f64>,
    normal_right: Vector3<f64>,
    /// Dihedral angle in radians (0 = coplanar, π = folded back).
    /// Used by fillet (Phase 2) to scale the arc profile.
    #[allow(dead_code)]
    dihedral: f64,
}

pub(super) const FEATURE_EDGE_DIHEDRAL_THRESHOLD: f64 = 0.35; // ~20 degrees

pub(super) fn detect_feature_edges(mesh: &IrMesh) -> Vec<FeatureEdge> {
    let tri_mesh = mesh.triangulate();
    let polygons = &tri_mesh.polygons;

    // Build vertex index map for epsilon-based deduplication.
    let mut vertex_map = csgrs::mesh::connectivity::VertexIndexMap::new(1e-9);
    for poly in polygons {
        for v in &poly.vertices {
            vertex_map.get_or_create_index(v.pos);
        }
    }

    // Map each canonical edge to the (up to two) polygon indices sharing it.
    let mut edge_faces: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (poly_idx, poly) in polygons.iter().enumerate() {
        let verts: Vec<usize> = poly
            .vertices
            .iter()
            .map(|v| vertex_map.get_or_create_index(v.pos))
            .collect();
        for i in 0..verts.len() {
            let a = verts[i];
            let b = verts[(i + 1) % verts.len()];
            let key = if a < b { (a, b) } else { (b, a) };
            edge_faces.entry(key).or_default().push(poly_idx);
        }
    }

    let mut result = Vec::new();
    for ((vi_a, vi_b), faces) in &edge_faces {
        if faces.len() != 2 {
            continue; // Non-manifold or boundary edge — skip.
        }
        let n1 = polygons[faces[0]].plane.normal();
        let n2 = polygons[faces[1]].plane.normal();
        let dot = n1.dot(&n2).clamp(-1.0, 1.0);
        let dihedral = dot.acos();
        if dihedral < FEATURE_EDGE_DIHEDRAL_THRESHOLD {
            continue; // Nearly coplanar — not a feature edge.
        }
        let pos_a = vertex_map
            .get_position(*vi_a)
            .expect("vertex index must exist");
        let pos_b = vertex_map
            .get_position(*vi_b)
            .expect("vertex index must exist");
        result.push(FeatureEdge {
            vi_a: *vi_a,
            vi_b: *vi_b,
            pos_a,
            pos_b,
            normal_left: n1,
            normal_right: n2,
            dihedral,
        });
    }
    result
}

pub(super) fn filter_edges(edges: &[FeatureEdge], selector: EdgeSelector) -> Vec<&FeatureEdge> {
    match selector {
        EdgeSelector::All => edges.iter().collect(),
        EdgeSelector::Top => {
            let max_z = edges
                .iter()
                .map(|e| e.pos_a.z.max(e.pos_b.z))
                .fold(f64::NEG_INFINITY, f64::max);
            let threshold = max_z - 1e-6;
            edges
                .iter()
                .filter(|e| {
                    let mid_z = (e.pos_a.z + e.pos_b.z) * 0.5;
                    mid_z >= threshold
                })
                .collect()
        }
        EdgeSelector::Bottom => {
            let min_z = edges
                .iter()
                .map(|e| e.pos_a.z.min(e.pos_b.z))
                .fold(f64::INFINITY, f64::min);
            let threshold = min_z + 1e-6;
            edges
                .iter()
                .filter(|e| {
                    let mid_z = (e.pos_a.z + e.pos_b.z) * 0.5;
                    mid_z <= threshold
                })
                .collect()
        }
        EdgeSelector::Vertical => edges
            .iter()
            .filter(|e| {
                let dir = (e.pos_b - e.pos_a).normalize();
                dir.z.abs() > 0.95
            })
            .collect(),
    }
}

pub(super) fn chamfer_mesh(
    mesh: &IrMesh,
    distance: f64,
    selector: EdgeSelector,
) -> AppResult<IrMesh> {
    if distance.abs() < 1e-9 {
        return Ok(mesh.clone());
    }
    let tri_mesh = mesh.triangulate();
    let all_edges = detect_feature_edges(&tri_mesh);
    let selected = filter_edges(&all_edges, selector);
    if selected.is_empty() {
        return Ok(mesh.clone());
    }

    // Build vertex index map matching the one used in detect_feature_edges.
    let polygons = &tri_mesh.polygons;
    let mut vertex_map = csgrs::mesh::connectivity::VertexIndexMap::new(1e-9);
    for poly in polygons {
        for v in &poly.vertices {
            vertex_map.get_or_create_index(v.pos);
        }
    }

    // Collect the set of selected edge keys.
    let selected_keys: std::collections::HashSet<(usize, usize)> =
        selected.iter().map(|e| (e.vi_a, e.vi_b)).collect();

    // For each selected edge, build a cutting plane that bevels the edge.
    // Strategy: for each triangle, if it has an edge in the selected set,
    // inset the edge by `distance` along the face plane and produce a chamfer
    // strip connecting the two faces.

    // Build edge → face normal pairs for selected edges.
    let mut edge_normals: HashMap<(usize, usize), (Vector3<f64>, Vector3<f64>)> = HashMap::new();
    for e in &selected {
        edge_normals.insert((e.vi_a, e.vi_b), (e.normal_left, e.normal_right));
    }

    // For each polygon, find which of its edges are selected and split accordingly.
    let mut out_polygons: Vec<IrPolygon<()>> = Vec::new();
    let mut chamfer_strips: Vec<[Point3<f64>; 4]> = Vec::new();

    // Track which polygon index was "left" or "right" for each edge so we
    // can assign inset directions consistently.
    let mut edge_face_sides: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (poly_idx, poly) in polygons.iter().enumerate() {
        let verts: Vec<usize> = poly
            .vertices
            .iter()
            .map(|v| vertex_map.get_or_create_index(v.pos))
            .collect();
        for i in 0..verts.len() {
            let a = verts[i];
            let b = verts[(i + 1) % verts.len()];
            let key = if a < b { (a, b) } else { (b, a) };
            if selected_keys.contains(&key) {
                edge_face_sides.entry(key).or_default().push(poly_idx);
            }
        }
    }

    // For each polygon, check if any of its edges are chamfered.
    // If so, inset those edge vertices along the face plane.
    for (poly_idx, poly) in polygons.iter().enumerate() {
        let face_normal = poly.plane.normal();
        let verts: Vec<usize> = poly
            .vertices
            .iter()
            .map(|v| vertex_map.get_or_create_index(v.pos))
            .collect();

        let mut has_chamfer_edge = false;
        let mut new_vertices: Vec<IrVertex> = Vec::new();

        for i in 0..verts.len() {
            let a = verts[i];
            let b = verts[(i + 1) % verts.len()];
            let key = if a < b { (a, b) } else { (b, a) };

            new_vertices.push(poly.vertices[i].clone());

            if selected_keys.contains(&key) {
                has_chamfer_edge = true;

                let pos_a = poly.vertices[i].pos;
                let pos_b = poly.vertices[(i + 1) % verts.len()].pos;
                let edge_dir = (pos_b - pos_a).normalize();

                // Inset direction: perpendicular to the edge within the face plane.
                let inset_dir = face_normal.cross(&edge_dir).normalize();
                // Ensure inset points inward (toward face interior).
                let face_center = Point3::from(
                    poly.vertices
                        .iter()
                        .fold(Vector3::zeros(), |acc, v| acc + v.pos.coords)
                        / poly.vertices.len() as f64,
                );
                let test_point = pos_a + inset_dir * 0.001;
                let inset_dir = if (test_point - face_center).norm() < (pos_a - face_center).norm()
                {
                    inset_dir
                } else {
                    -inset_dir
                };

                let inset_a = pos_a + inset_dir * distance;
                let inset_b = pos_b + inset_dir * distance;

                // Replace the original edge endpoints with inset versions.
                // We add the inset points and will later form the chamfer strip.
                let last_idx = new_vertices.len() - 1;
                new_vertices[last_idx] = IrVertex::new(inset_a, face_normal);
                new_vertices.push(IrVertex::new(inset_b, face_normal));

                // Record the chamfer strip quad: original → inset on both sides.
                // We only record from one side; the strip connects when both faces
                // have been processed. Use edge_face_sides to determine if this is
                // the first or second face.
                let sides = edge_face_sides.get(&key).unwrap();
                if sides.len() == 2 && sides[0] == poly_idx {
                    // First face records the strip — we'll get the other side's
                    // inset points from the second face processing below.
                    chamfer_strips.push([pos_a, pos_b, inset_b, inset_a]);
                }
            }
        }

        if has_chamfer_edge && new_vertices.len() >= 3 {
            out_polygons.push(IrPolygon::new(new_vertices, None));
        } else {
            out_polygons.push(poly.clone());
        }
    }

    // Now build the chamfer strip polygons connecting the two inset faces.
    // For each selected edge, we need to form a quad from the inset points
    // on both adjacent faces.
    for e in &selected {
        let key = (e.vi_a, e.vi_b);
        let sides = match edge_face_sides.get(&key) {
            Some(s) if s.len() == 2 => s,
            _ => continue,
        };

        // Get inset positions from both faces.
        let poly_l = &polygons[sides[0]];
        let poly_r = &polygons[sides[1]];
        let nl = poly_l.plane.normal();
        let nr = poly_r.plane.normal();

        let mut compute_inset =
            |poly: &IrPolygon<()>, normal: Vector3<f64>| -> (Point3<f64>, Point3<f64>) {
                let verts: Vec<usize> = poly
                    .vertices
                    .iter()
                    .map(|v| vertex_map.get_or_create_index(v.pos))
                    .collect();
                // Find the edge in this polygon.
                for i in 0..verts.len() {
                    let a = verts[i];
                    let b = verts[(i + 1) % verts.len()];
                    let k = if a < b { (a, b) } else { (b, a) };
                    if k == key {
                        let pos_a = poly.vertices[i].pos;
                        let pos_b = poly.vertices[(i + 1) % verts.len()].pos;
                        let edge_dir = (pos_b - pos_a).normalize();
                        let mut inset_dir = normal.cross(&edge_dir).normalize();
                        let face_center = Point3::from(
                            poly.vertices
                                .iter()
                                .fold(Vector3::zeros(), |acc, v| acc + v.pos.coords)
                                / poly.vertices.len() as f64,
                        );
                        let test_point = pos_a + inset_dir * 0.001;
                        if (test_point - face_center).norm() > (pos_a - face_center).norm() {
                            inset_dir = -inset_dir;
                        }
                        return (pos_a + inset_dir * distance, pos_b + inset_dir * distance);
                    }
                }
                (e.pos_a, e.pos_b) // fallback — shouldn't happen
            };

        let (inset_la, inset_lb) = compute_inset(poly_l, nl);
        let (inset_ra, inset_rb) = compute_inset(poly_r, nr);

        // The chamfer strip quad connects:
        //   inset_la — inset_lb — inset_rb — inset_ra
        // But we need to be careful about winding order for correct normals.
        let edge_vec: Vector3<f64> = inset_lb - inset_la;
        let side_vec: Vector3<f64> = inset_ra - inset_la;
        let chamfer_normal = edge_vec.cross(&side_vec).normalize();
        // Check if normal points outward (same hemisphere as average of face normals).
        let avg_outward = (nl + nr).normalize();
        let strip_verts = if chamfer_normal.dot(&avg_outward) > 0.0 {
            vec![
                IrVertex::new(inset_la, chamfer_normal),
                IrVertex::new(inset_lb, chamfer_normal),
                IrVertex::new(inset_rb, chamfer_normal),
                IrVertex::new(inset_ra, chamfer_normal),
            ]
        } else {
            let flipped = -chamfer_normal;
            vec![
                IrVertex::new(inset_ra, flipped),
                IrVertex::new(inset_rb, flipped),
                IrVertex::new(inset_lb, flipped),
                IrVertex::new(inset_la, flipped),
            ]
        };
        out_polygons.push(IrPolygon::new(strip_verts, None));
    }

    Ok(IrMesh::from_polygons(&out_polygons, None))
}

pub(super) fn polygon_inset_for_edge(
    poly: &IrPolygon<()>,
    key: (usize, usize),
    vertex_map: &mut csgrs::mesh::connectivity::VertexIndexMap,
    distance: f64,
) -> Option<(Point3<f64>, Point3<f64>, Vector3<f64>)> {
    let verts: Vec<usize> = poly
        .vertices
        .iter()
        .map(|v| vertex_map.get_or_create_index(v.pos))
        .collect();
    for i in 0..verts.len() {
        let a = verts[i];
        let b = verts[(i + 1) % verts.len()];
        let k = if a < b { (a, b) } else { (b, a) };
        if k != key {
            continue;
        }
        let pos_a = poly.vertices[i].pos;
        let pos_b = poly.vertices[(i + 1) % verts.len()].pos;
        let edge_dir = (pos_b - pos_a).normalize();
        let face_normal = poly.plane.normal();
        let mut inset_dir = face_normal.cross(&edge_dir).normalize();
        let face_center = Point3::from(
            poly.vertices
                .iter()
                .fold(Vector3::zeros(), |acc, v| acc + v.pos.coords)
                / poly.vertices.len() as f64,
        );
        let test_point = pos_a + inset_dir * 0.001;
        if (test_point - face_center).norm() > (pos_a - face_center).norm() {
            inset_dir = -inset_dir;
        }
        return Some((
            pos_a + inset_dir * distance,
            pos_b + inset_dir * distance,
            inset_dir,
        ));
    }
    None
}

pub(super) fn fillet_inset_distance(radius: f64, dihedral: f64) -> f64 {
    let half = (dihedral * 0.5).clamp(1e-4, std::f64::consts::FRAC_PI_2 - 1e-4);
    radius / half.tan()
}

pub(super) fn fillet_segment_count(radius: f64, dihedral: f64) -> usize {
    let density = (radius.abs() * dihedral.max(0.5)).ceil() as usize;
    density.clamp(4, 10)
}

pub(super) fn rotate_around_axis(v: Vector3<f64>, axis: Vector3<f64>, angle: f64) -> Vector3<f64> {
    let unit_axis = axis.normalize();
    let cos = angle.cos();
    let sin = angle.sin();
    v * cos + unit_axis.cross(&v) * sin + unit_axis * unit_axis.dot(&v) * (1.0 - cos)
}

pub(super) fn fillet_arc_points(
    corner: Point3<f64>,
    start_dir: Vector3<f64>,
    end_dir: Vector3<f64>,
    axis: Vector3<f64>,
    radius: f64,
    dihedral: f64,
    segments: usize,
) -> Option<Vec<(Point3<f64>, Vector3<f64>)>> {
    let bisector = (start_dir + end_dir).try_normalize(1e-9)?;
    let center = corner + bisector * (radius / (dihedral * 0.5).sin());
    let tangent_distance = fillet_inset_distance(radius, dihedral);
    let tangent_start = corner + start_dir * tangent_distance;
    let tangent_end = corner + end_dir * tangent_distance;
    let radial_start = tangent_start - center;
    let radial_end = tangent_end - center;
    let axis = axis.try_normalize(1e-9)?;
    let mut angle = radial_start
        .normalize()
        .dot(&radial_end.normalize())
        .clamp(-1.0, 1.0)
        .acos();
    if radial_start.cross(&radial_end).dot(&axis) < 0.0 {
        angle = -angle;
    }
    let mut points = Vec::with_capacity(segments + 1);
    for step in 0..=segments {
        let t = step as f64 / segments as f64;
        let radial = rotate_around_axis(radial_start, axis, angle * t);
        let point = center + radial;
        points.push((point, radial.normalize()));
    }
    Some(points)
}

pub(super) fn fillet_mesh(mesh: &IrMesh, radius: f64, selector: EdgeSelector) -> AppResult<IrMesh> {
    if radius.abs() < 1e-9 {
        return Ok(mesh.clone());
    }
    let tri_mesh = mesh.triangulate();
    let all_edges = detect_feature_edges(&tri_mesh);
    let selected = filter_edges(&all_edges, selector);
    if selected.is_empty() {
        return Ok(mesh.clone());
    }

    let polygons = &tri_mesh.polygons;
    let mut vertex_map = csgrs::mesh::connectivity::VertexIndexMap::new(1e-9);
    for poly in polygons {
        for v in &poly.vertices {
            vertex_map.get_or_create_index(v.pos);
        }
    }

    let edge_distances: HashMap<(usize, usize), f64> = selected
        .iter()
        .map(|edge| {
            (
                (edge.vi_a, edge.vi_b),
                fillet_inset_distance(radius, edge.dihedral),
            )
        })
        .collect();
    let selected_keys: std::collections::HashSet<(usize, usize)> =
        edge_distances.keys().copied().collect();

    let mut edge_face_sides: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (poly_idx, poly) in polygons.iter().enumerate() {
        let verts: Vec<usize> = poly
            .vertices
            .iter()
            .map(|v| vertex_map.get_or_create_index(v.pos))
            .collect();
        for i in 0..verts.len() {
            let a = verts[i];
            let b = verts[(i + 1) % verts.len()];
            let key = if a < b { (a, b) } else { (b, a) };
            if selected_keys.contains(&key) {
                edge_face_sides.entry(key).or_default().push(poly_idx);
            }
        }
    }

    let mut out_polygons: Vec<IrPolygon<()>> = Vec::new();
    for poly in polygons {
        let verts: Vec<usize> = poly
            .vertices
            .iter()
            .map(|v| vertex_map.get_or_create_index(v.pos))
            .collect();
        let face_normal = poly.plane.normal();
        let mut modified = false;
        let mut new_vertices = Vec::new();

        for i in 0..verts.len() {
            let a = verts[i];
            let b = verts[(i + 1) % verts.len()];
            let key = if a < b { (a, b) } else { (b, a) };
            new_vertices.push(poly.vertices[i].clone());
            let Some(distance) = edge_distances.get(&key).copied() else {
                continue;
            };
            modified = true;
            let Some((inset_a, inset_b, _)) =
                polygon_inset_for_edge(poly, key, &mut vertex_map, distance)
            else {
                continue;
            };
            let last_idx = new_vertices.len() - 1;
            new_vertices[last_idx] = IrVertex::new(inset_a, face_normal);
            new_vertices.push(IrVertex::new(inset_b, face_normal));
        }

        if modified && new_vertices.len() >= 3 {
            out_polygons.push(IrPolygon::new(new_vertices, None));
        } else {
            out_polygons.push(poly.clone());
        }
    }

    for edge in selected {
        let key = (edge.vi_a, edge.vi_b);
        let Some(sides) = edge_face_sides.get(&key) else {
            continue;
        };
        if sides.len() != 2 {
            continue;
        }
        let Some(distance) = edge_distances.get(&key).copied() else {
            continue;
        };
        let poly_l = &polygons[sides[0]];
        let poly_r = &polygons[sides[1]];
        let Some((_inset_la, _inset_lb, dir_l)) =
            polygon_inset_for_edge(poly_l, key, &mut vertex_map, distance)
        else {
            continue;
        };
        let Some((_inset_ra, _inset_rb, dir_r)) =
            polygon_inset_for_edge(poly_r, key, &mut vertex_map, distance)
        else {
            continue;
        };
        let edge_axis = edge.pos_b - edge.pos_a;
        let segments = fillet_segment_count(radius, edge.dihedral);
        let Some(arc_a) = fillet_arc_points(
            edge.pos_a,
            dir_l,
            dir_r,
            edge_axis,
            radius,
            edge.dihedral,
            segments,
        ) else {
            continue;
        };
        let Some(arc_b) = fillet_arc_points(
            edge.pos_b,
            dir_l,
            dir_r,
            edge_axis,
            radius,
            edge.dihedral,
            segments,
        ) else {
            continue;
        };

        let avg_outward = (edge.normal_left + edge.normal_right)
            .try_normalize(1e-9)
            .unwrap_or(edge.normal_left);
        for segment in 0..segments {
            let (a0, n_a0) = arc_a[segment];
            let (a1, n_a1) = arc_a[segment + 1];
            let (b0, n_b0) = arc_b[segment];
            let (b1, n_b1) = arc_b[segment + 1];
            let mut poly = IrPolygon::new(
                vec![
                    IrVertex::new(a0, n_a0),
                    IrVertex::new(b0, n_b0),
                    IrVertex::new(b1, n_b1),
                    IrVertex::new(a1, n_a1),
                ],
                None,
            );
            if poly.plane.normal().dot(&avg_outward) < 0.0 {
                poly.flip();
            }
            out_polygons.push(poly);
        }
    }

    Ok(IrMesh::from_polygons(&out_polygons, None))
}

pub(super) fn parse_edge_selector(
    args: &[IrExpr],
    env: &BTreeMap<String, ParamValue>,
) -> AppResult<(EdgeSelector, usize)> {
    // Check for :edges "selector" keyword pair after the distance argument.
    if args.len() >= 3
        && expr_keyword_name(&args[1])
            .map(|k| k == "edges")
            .unwrap_or(false)
    {
        let selector_str = eval_stringish(&args[2], env)?;
        let selector = match selector_str.as_str() {
            "all" => EdgeSelector::All,
            "top" => EdgeSelector::Top,
            "bottom" => EdgeSelector::Bottom,
            "vertical" => EdgeSelector::Vertical,
            other => {
                return Err(validation(format!(
                    "Unknown edge selector `{}`. Use `all`, `top`, `bottom`, or `vertical`.",
                    other
                )));
            }
        };
        Ok((selector, 3))
    } else {
        Ok((EdgeSelector::All, 1))
    }
}
