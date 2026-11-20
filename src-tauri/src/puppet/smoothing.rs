use super::MeshAsset;
use std::collections::HashMap;

type Point = [f64; 3];

pub(super) struct SmoothedMesh {
    pub corners: Vec<usize>,
    pub vertices: Vec<SmoothedVertex>,
}

pub(super) struct SmoothedVertex {
    pub position: [f32; 3],
    pub normal: Option<[f32; 3]>,
}

struct Vertex {
    original: Point,
    normal: Point,
    neighbor_sum: Point,
    neighbor_count: u32,
    limit: f64,
}

fn subtract(a: Point, b: Point) -> Point {
    std::array::from_fn(|i| a[i] - b[i])
}

fn dot(a: Point, b: Point) -> f64 {
    a.into_iter().zip(b).map(|(a, b)| a * b).sum()
}

fn length(vector: Point) -> f64 {
    dot(vector, vector).sqrt()
}

fn face_normal([a, b, c]: [Point; 3]) -> Point {
    let u = subtract(b, a);
    let v = subtract(c, a);
    [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ]
}

fn accumulate(sum: &mut Point, value: Point) {
    for (component, value) in sum.iter_mut().zip(value) {
        *component += value;
    }
}

pub(super) fn smooth(asset: &MeshAsset, limit_mm: f64) -> Option<SmoothedMesh> {
    if limit_mm == 0.0 {
        return None;
    }
    // OBJ UV/material splits and STL triangle corners can share a position.
    // Use one displacement per position so those seams stay joined.
    let mut by_position = HashMap::new();
    let mut vertices = Vec::new();
    let corners: Vec<_> = asset
        .primitives
        .iter()
        .flat_map(|primitive| &primitive.positions)
        .map(|position| {
            let key = position.map(|v| if v == 0.0 { 0 } else { v.to_bits() });
            *by_position.entry(key).or_insert_with(|| {
                let index = vertices.len();
                vertices.push(Vertex {
                    original: position.map(f64::from),
                    normal: [0.0; 3],
                    neighbor_sum: [0.0; 3],
                    neighbor_count: 0,
                    limit: limit_mm,
                });
                index
            })
        })
        .collect();
    drop(by_position);

    let faces = corners.as_chunks::<3>().0;
    let mut edges = Vec::with_capacity(corners.len());
    for &face in faces {
        let points = face.map(|index| vertices[index].original);
        let normal = face_normal(points);
        let double_area = length(normal);
        for i in 0..3 {
            let mut edge = [face[(i + 1) % 3], face[(i + 2) % 3]];
            edge.sort_unstable();
            edges.push(edge);
            let opposite = length(subtract(points[(i + 1) % 3], points[(i + 2) % 3]));
            let vertex = &mut vertices[face[i]];
            // On a narrow triangle, even a small move can erase a detail.
            // Bound each corner by a fraction of its height above the opposite edge.
            let local_limit = if opposite > 0.0 {
                double_area / opposite / 8.0
            } else {
                0.0
            };
            vertex.limit = vertex.limit.min(local_limit);
            accumulate(&mut vertex.normal, normal);
        }
    }
    edges.sort_unstable();
    for incidences in edges.chunk_by(|a, b| a == b) {
        let [a, b] = incidences[0];
        if a == b || incidences.len() != 2 {
            vertices[a].limit = 0.0;
            vertices[b].limit = 0.0;
            continue;
        }
        for (from, to) in [(a, b), (b, a)] {
            let neighbor = vertices[to].original;
            accumulate(&mut vertices[from].neighbor_sum, neighbor);
            vertices[from].neighbor_count += 1;
        }
    }
    drop(edges);

    let positions: Vec<_> = vertices
        .iter()
        .map(|vertex| {
            let normal_length = length(vertex.normal);
            if vertex.limit == 0.0 || vertex.neighbor_count == 0 || normal_length == 0.0 {
                return vertex.original;
            }
            let normal = vertex.normal.map(|value| value / normal_length);
            let average = vertex
                .neighbor_sum
                .map(|value| value / f64::from(vertex.neighbor_count));
            let offset =
                dot(subtract(average, vertex.original), normal).clamp(-vertex.limit, vertex.limit);
            // A normal displacement reduces ripples without redistributing flat triangles.
            std::array::from_fn(|i| vertex.original[i] + normal[i] * offset)
        })
        .collect();

    let mut normals = vec![[0.0; 3]; positions.len()];
    for &face in faces {
        let normal = face_normal(face.map(|index| positions[index]));
        for index in face {
            accumulate(&mut normals[index], normal);
        }
    }
    let vertices = positions
        .into_iter()
        .zip(normals)
        .zip(vertices)
        .map(|((position, normal), original)| {
            let position = position.map(|value| value as f32);
            let normal_length = length(normal);
            let normal = (position != original.original.map(|value| value as f32)
                && normal_length > 0.0)
                .then(|| normal.map(|value| (value / normal_length) as f32));
            SmoothedVertex { position, normal }
        })
        .collect();
    Some(SmoothedMesh { corners, vertices })
}
