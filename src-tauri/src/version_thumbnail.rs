//! Durable, viewport-independent thumbnail of the exact persisted render mesh.
use std::path::Path;

use base64::Engine;
use image::{ImageFormat, Rgb, RgbImage};
use std::io::Cursor;

const WIDTH: u32 = 320;
const HEIGHT: u32 = 240;
const BACKGROUND: [u8; 3] = [10, 13, 22];

pub(crate) fn render_stl(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let triangles = crate::services::printability::parse_stl_triangles(&bytes)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if triangles.is_empty() {
        return Err(format!("{}: STL contains no triangles", path.display()));
    }
    let cross = |u: [f64; 3], v: [f64; 3]| {
        [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ]
    };
    let norm = |v: [f64; 3]| v.iter().map(|x| x * x).sum::<f64>().sqrt();
    let dot = |u: [f64; 3], v: [f64; 3]| u.iter().zip(v).map(|(a, b)| a * b).sum::<f64>();
    // Fixed isometric camera, with a face-on fallback for an edge-on planar mesh.
    let mut camera = [
        [1.0 / 2.0_f64.sqrt(), -1.0 / 2.0_f64.sqrt(), 0.0],
        [
            1.0 / 6.0_f64.sqrt(),
            1.0 / 6.0_f64.sqrt(),
            -2.0 / 6.0_f64.sqrt(),
        ],
        [1.0 / 3.0_f64.sqrt(); 3],
    ];
    let normals = triangles
        .iter()
        .map(|triangle| {
            let [a, b, c] = triangle.map(|p| p.map(f64::from));
            cross(
                std::array::from_fn(|i| b[i] - a[i]),
                std::array::from_fn(|i| c[i] - a[i]),
            )
        })
        .collect::<Vec<_>>();
    if !normals
        .iter()
        .any(|normal| dot(*normal, camera[2]).abs() > norm(*normal) * 1.0e-9)
    {
        let normal = normals
            .iter()
            .max_by(|a, b| norm(**a).total_cmp(&norm(**b)))
            .copied()
            .unwrap();
        let length = norm(normal);
        if length <= f64::EPSILON {
            return Err(format!(
                "{}: STL contains only degenerate triangles",
                path.display()
            ));
        }
        let direction = normal.map(|value| value / length);
        let up = if direction[2].abs() > 0.9 {
            [0.0, 1.0, 0.0]
        } else {
            [0.0, 0.0, 1.0]
        };
        let right = cross(direction, up);
        let right = right.map(|value| value / norm(right));
        camera = [right, cross(direction, right), direction];
    }
    let project =
        |point: [f32; 3]| -> [f64; 3] { camera.map(|axis| dot(axis, point.map(f64::from))) };
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for point in triangles.iter().flatten() {
        if !point.iter().all(|value| value.is_finite()) {
            return Err(format!(
                "{}: STL contains non-finite coordinates",
                path.display()
            ));
        }
        let point = project(*point);
        for axis in 0..2 {
            min[axis] = min[axis].min(point[axis]);
            max[axis] = max[axis].max(point[axis]);
        }
    }
    let extent = [max[0] - min[0], max[1] - min[1]];
    if extent[0].max(extent[1]) <= f64::EPSILON {
        return Err(format!("{}: STL has no visible extent", path.display()));
    }
    let scale = ((WIDTH as f64 - 32.0) / extent[0].max(f64::EPSILON))
        .min((HEIGHT as f64 - 32.0) / extent[1].max(f64::EPSILON));
    let mut image = RgbImage::from_pixel(WIDTH, HEIGHT, Rgb(BACKGROUND));
    let mut depth = vec![f64::NEG_INFINITY; (WIDTH * HEIGHT) as usize];
    let edge = |a: [f64; 3], b: [f64; 3], x: f64, y: f64| {
        (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0])
    };
    let mut painted = false;
    for triangle in triangles {
        let points = triangle.map(|point| {
            let mut point = project(point);
            point[0] = (point[0] - (min[0] + max[0]) / 2.0) * scale + WIDTH as f64 / 2.0;
            point[1] = (point[1] - (min[1] + max[1]) / 2.0) * scale + HEIGHT as f64 / 2.0;
            point
        });
        let [a, b, c] = points;
        let area = edge(a, b, c[0], c[1]);
        if area.abs() < 1.0e-10 {
            continue;
        }
        let u = [b[0] - a[0], b[1] - a[1], (b[2] - a[2]) * scale];
        let v = [c[0] - a[0], c[1] - a[1], (c[2] - a[2]) * scale];
        let normal = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let length = normal.iter().map(|value| value * value).sum::<f64>().sqrt();
        let light = ((normal[0] * -0.35 + normal[1] * -0.45 + normal[2] * 0.82) / length).abs();
        let shade = 0.42 + 0.58 * light.min(1.0);
        let color = Rgb([190.0, 159.0, 110.0].map(|channel| (channel * shade) as u8));
        let bounds = |axis: usize, limit: u32| -> (u32, u32) {
            let low = points.iter().map(|p| p[axis]).fold(f64::INFINITY, f64::min);
            let high = points
                .iter()
                .map(|p| p[axis])
                .fold(f64::NEG_INFINITY, f64::max);
            (
                low.floor().clamp(0.0, (limit - 1) as f64) as u32,
                high.ceil().clamp(0.0, (limit - 1) as f64) as u32,
            )
        };
        let (x0, x1) = bounds(0, WIDTH);
        let (y0, y1) = bounds(1, HEIGHT);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let px = x as f64 + 0.5;
                let py = y as f64 + 0.5;
                let weights = [
                    edge(b, c, px, py) / area,
                    edge(c, a, px, py) / area,
                    edge(a, b, px, py) / area,
                ];
                if weights.iter().any(|weight| *weight < -1.0e-9) {
                    continue;
                }
                let z = weights[0] * a[2] + weights[1] * b[2] + weights[2] * c[2];
                let index = (y * WIDTH + x) as usize;
                if z > depth[index] {
                    depth[index] = z;
                    image.put_pixel(x, y, color);
                    painted = true;
                }
            }
        }
    }
    if !painted {
        return Err(format!(
            "{}: STL contains no visible triangles",
            path.display()
        ));
    }
    let mut png = Cursor::new(Vec::new());
    image
        .write_to(&mut png, ImageFormat::Png)
        .map_err(|error| format!("Thumbnail PNG: {error}"))?;
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png.into_inner())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn actual_geometry_produces_fitted_bronze_png() {
        let path = std::env::temp_dir().join(format!("thumbnail-{}.stl", uuid::Uuid::new_v4()));
        std::fs::write(&path, "solid mesh\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 100 0 0\nvertex 0 50 20\nendloop\nendfacet\nendsolid mesh").unwrap();
        let data = render_stl(&path).unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data.strip_prefix("data:image/png;base64,").unwrap())
            .unwrap();
        let image = image::load_from_memory(&bytes).unwrap().to_rgb8();
        assert_eq!(image.dimensions(), (320, 240));
        let lit = image
            .pixels()
            .filter(|pixel| pixel.0 != [10, 13, 22])
            .count();
        assert!(
            lit > 1000,
            "geometry must occupy thumbnail, got {lit} pixels"
        );
        assert_eq!(image.get_pixel(0, 0).0, [10, 13, 22]);
        assert_eq!(
            render_stl(&path).unwrap(),
            data,
            "same mesh produces same PNG"
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn plane_edge_on_to_default_camera_still_produces_visible_geometry() {
        let path =
            std::env::temp_dir().join(format!("thumbnail-plane-{}.stl", uuid::Uuid::new_v4()));
        std::fs::write(&path, "solid mesh\nfacet normal 0 -1 1\nouter loop\nvertex 0 0 0\nvertex 10 0 0\nvertex 0 10 10\nendloop\nendfacet\nendsolid mesh").unwrap();
        let data = render_stl(&path).expect("edge-on plane needs fitted alternate camera");
        assert!(data.starts_with("data:image/png;base64,"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn unreadable_or_empty_mesh_returns_raw_error_instead_of_blank_png() {
        let path = std::env::temp_dir().join(format!("thumbnail-{}.stl", uuid::Uuid::new_v4()));
        assert!(render_stl(&path).is_err());
        std::fs::write(&path, "solid empty\nendsolid empty").unwrap();
        assert!(render_stl(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
