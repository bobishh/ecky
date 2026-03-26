use crate::contracts::{DisplacementSpec, ProjectionType};
use crate::models::{AppError, AppResult};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

#[derive(Clone, Copy, Debug)]
struct Vec3 {
    x: f32,
    y: f32,
    z: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axis {
    X,
    Y,
    Z,
}

impl Vec3 {
    fn add(&self, other: &Vec3) -> Vec3 {
        Vec3 {
            x: self.x + other.x,
            y: self.y + other.y,
            z: self.z + other.z,
        }
    }
    fn sub(&self, other: &Vec3) -> Vec3 {
        Vec3 {
            x: self.x - other.x,
            y: self.y - other.y,
            z: self.z - other.z,
        }
    }
    fn mul(&self, s: f32) -> Vec3 {
        Vec3 {
            x: self.x * s,
            y: self.y * s,
            z: self.z * s,
        }
    }
    fn cross(&self, other: &Vec3) -> Vec3 {
        Vec3 {
            x: self.y * other.z - self.z * other.y,
            y: self.z * other.x - self.x * other.z,
            z: self.x * other.y - self.y * other.x,
        }
    }
    fn normalize(&self) -> Vec3 {
        let len = (self.x * self.x + self.y * self.y + self.z * self.z).sqrt();
        if len > 1e-8 {
            self.mul(1.0 / len)
        } else {
            *self
        }
    }
}

fn read_f32<R: Read>(reader: &mut R) -> std::io::Result<f32> {
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf)?;
    Ok(f32::from_le_bytes(buf))
}

fn read_vec3<R: Read>(reader: &mut R) -> std::io::Result<Vec3> {
    Ok(Vec3 {
        x: read_f32(reader)?,
        y: read_f32(reader)?,
        z: read_f32(reader)?,
    })
}

fn write_f32<W: Write>(writer: &mut W, v: f32) -> std::io::Result<()> {
    writer.write_all(&v.to_le_bytes())
}

fn write_vec3<W: Write>(writer: &mut W, v: &Vec3) -> std::io::Result<()> {
    write_f32(writer, v.x)?;
    write_f32(writer, v.y)?;
    write_f32(writer, v.z)
}

fn quantize(v: &Vec3) -> (i32, i32, i32) {
    (
        (v.x * 1000.0).round() as i32,
        (v.y * 1000.0).round() as i32,
        (v.z * 1000.0).round() as i32,
    )
}

fn axis_value(v: &Vec3, axis: Axis) -> f32 {
    match axis {
        Axis::X => v.x,
        Axis::Y => v.y,
        Axis::Z => v.z,
    }
}

fn axis_unit(axis: Axis) -> Vec3 {
    match axis {
        Axis::X => Vec3 {
            x: 1.0,
            y: 0.0,
            z: 0.0,
        },
        Axis::Y => Vec3 {
            x: 0.0,
            y: 1.0,
            z: 0.0,
        },
        Axis::Z => Vec3 {
            x: 0.0,
            y: 0.0,
            z: 1.0,
        },
    }
}

fn choose_planar_axes(
    min_x: f32,
    max_x: f32,
    min_y: f32,
    max_y: f32,
    min_z: f32,
    max_z: f32,
) -> (Axis, Axis) {
    let mut spans = [
        (Axis::X, max_x - min_x),
        (Axis::Y, max_y - min_y),
        (Axis::Z, max_z - min_z),
    ];
    spans.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    (spans[0].0, spans[1].0)
}

fn remaining_axis(planar_axes: (Axis, Axis)) -> Axis {
    match planar_axes {
        (Axis::X, Axis::Y) | (Axis::Y, Axis::X) => Axis::Z,
        (Axis::X, Axis::Z) | (Axis::Z, Axis::X) => Axis::Y,
        (Axis::Y, Axis::Z) | (Axis::Z, Axis::Y) => Axis::X,
        _ => Axis::Z,
    }
}

#[derive(Clone, Copy, Debug)]
struct PlanarProjectionContext {
    axes: (Axis, Axis),
    min_x: f32,
    max_x: f32,
    min_y: f32,
    max_y: f32,
    min_z: f32,
    max_z: f32,
}

impl PlanarProjectionContext {
    fn new(
        axes: (Axis, Axis),
        min_x: f32,
        max_x: f32,
        min_y: f32,
        max_y: f32,
        min_z: f32,
        max_z: f32,
    ) -> Self {
        Self {
            axes,
            min_x,
            max_x,
            min_y,
            max_y,
            min_z,
            max_z,
        }
    }

    fn axis_bounds(&self, axis: Axis) -> (f32, f32) {
        match axis {
            Axis::X => (self.min_x, self.max_x),
            Axis::Y => (self.min_y, self.max_y),
            Axis::Z => (self.min_z, self.max_z),
        }
    }

    fn uv(&self, v: &Vec3) -> (f32, f32) {
        let (u_min, u_max) = self.axis_bounds(self.axes.0);
        let (v_min, v_max) = self.axis_bounds(self.axes.1);
        let u_span = u_max - u_min;
        let v_span = v_max - v_min;
        let u = if u_span > 1e-5 {
            (axis_value(v, self.axes.0) - u_min) / u_span
        } else {
            0.0
        };
        let v_tex = if v_span > 1e-5 {
            (axis_value(v, self.axes.1) - v_min) / v_span
        } else {
            0.0
        };
        (u, v_tex)
    }

    fn normal_axis(&self) -> Axis {
        remaining_axis(self.axes)
    }
}

fn edge_key(a: usize, b: usize) -> (usize, usize) {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}

fn midpoint(v1: &Vec3, v2: &Vec3) -> Vec3 {
    Vec3 {
        x: (v1.x + v2.x) * 0.5,
        y: (v1.y + v2.y) * 0.5,
        z: (v1.z + v2.z) * 0.5,
    }
}

fn max_planar_edge_span(
    tri: &[usize; 3],
    vertices: &[Vec3],
    projection: &PlanarProjectionContext,
) -> f32 {
    let uv0 = projection.uv(&vertices[tri[0]]);
    let uv1 = projection.uv(&vertices[tri[1]]);
    let uv2 = projection.uv(&vertices[tri[2]]);
    [(uv0, uv1), (uv1, uv2), (uv2, uv0)]
        .into_iter()
        .map(|(left, right)| (left.0 - right.0).abs().max((left.1 - right.1).abs()))
        .fold(0.0, f32::max)
}

fn refine_planar_mesh(
    vertices: &mut Vec<Vec3>,
    indices: &mut Vec<[usize; 3]>,
    projection: &PlanarProjectionContext,
    image_width: u32,
    image_height: u32,
) {
    let (u_axis, v_axis) = projection.axes;
    let (u_min, u_max) = projection.axis_bounds(u_axis);
    let (v_min, v_max) = projection.axis_bounds(v_axis);
    let longest_mm = (u_max - u_min).abs().max((v_max - v_min).abs()).max(1.0);
    let target_segments = (image_width.max(image_height) as f32)
        .min((longest_mm * 8.0).max(96.0))
        .clamp(96.0, 1024.0);
    let uv_edge_limit = 1.0 / target_segments;

    for _ in 0..8 {
        let mut midpoint_cache: HashMap<(usize, usize), usize> = HashMap::new();
        let mut next_indices = Vec::with_capacity(indices.len());
        let mut split_count = 0usize;

        for tri in indices.iter() {
            let max_edge_span = max_planar_edge_span(tri, vertices, projection);

            if max_edge_span <= uv_edge_limit {
                next_indices.push(*tri);
                continue;
            }

            split_count += 1;

            let midpoint_index = |left: usize,
                                  right: usize,
                                  vertices: &mut Vec<Vec3>,
                                  midpoint_cache: &mut HashMap<(usize, usize), usize>|
             -> usize {
                let key = edge_key(left, right);
                if let Some(existing) = midpoint_cache.get(&key) {
                    return *existing;
                }
                let idx = vertices.len();
                let point = midpoint(&vertices[left], &vertices[right]);
                vertices.push(point);
                midpoint_cache.insert(key, idx);
                idx
            };

            let a = tri[0];
            let b = tri[1];
            let c = tri[2];
            let ab = midpoint_index(a, b, vertices, &mut midpoint_cache);
            let bc = midpoint_index(b, c, vertices, &mut midpoint_cache);
            let ca = midpoint_index(c, a, vertices, &mut midpoint_cache);

            next_indices.push([a, ab, ca]);
            next_indices.push([ab, b, bc]);
            next_indices.push([ca, bc, c]);
            next_indices.push([ab, bc, ca]);
        }

        *indices = next_indices;

        if split_count == 0 {
            break;
        }
    }
}

fn sample_luma_bilinear(img: &image::GrayImage, u: f32, v: f32) -> f32 {
    let img_w = img.width().max(1);
    let img_h = img.height().max(1);
    let x = u.clamp(0.0, 1.0) * (img_w as f32 - 1.0);
    let y = v.clamp(0.0, 1.0) * (img_h as f32 - 1.0);
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(img_w - 1);
    let y1 = (y0 + 1).min(img_h - 1);
    let tx = x - x0 as f32;
    let ty = y - y0 as f32;
    let p00 = img.get_pixel(x0, y0).0[0] as f32 / 255.0;
    let p10 = img.get_pixel(x1, y0).0[0] as f32 / 255.0;
    let p01 = img.get_pixel(x0, y1).0[0] as f32 / 255.0;
    let p11 = img.get_pixel(x1, y1).0[0] as f32 / 255.0;
    let top = p00 + (p10 - p00) * tx;
    let bottom = p01 + (p11 - p01) * tx;
    top + (bottom - top) * ty
}

pub fn apply(
    input_stl: &Path,
    image_path: &str,
    spec: &DisplacementSpec,
    output_stl: &Path,
) -> AppResult<()> {
    let img = image::open(image_path)
        .map_err(|e| AppError::internal(format!("Failed to open image for displacement: {}", e)))?
        .into_luma8();

    let (img_w, img_h) = img.dimensions();

    let mut file = File::open(input_stl)
        .map_err(|e| AppError::internal(format!("Failed to open input STL: {}", e)))?;

    let mut header = [0u8; 80];
    file.read_exact(&mut header)
        .map_err(|e| AppError::internal(format!("Failed to read STL header: {}", e)))?;

    let mut count_buf = [0u8; 4];
    file.read_exact(&mut count_buf)
        .map_err(|e| AppError::internal(format!("Failed to read STL count: {}", e)))?;
    let num_triangles = u32::from_le_bytes(count_buf);

    let mut vertices: Vec<Vec3> = Vec::new();
    let mut vertex_normals: Vec<Vec3> = Vec::new();
    let mut vertex_map: HashMap<(i32, i32, i32), usize> = HashMap::new();
    let mut indices: Vec<[usize; 3]> = Vec::with_capacity(num_triangles as usize);

    for _ in 0..num_triangles {
        let normal = read_vec3(&mut file).unwrap_or(Vec3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        });
        let mut tri_indices = [0; 3];
        for tri_index in &mut tri_indices {
            let v = read_vec3(&mut file)
                .map_err(|e| AppError::internal(format!("Failed to read vertex: {}", e)))?;
            let q = quantize(&v);
            let idx = *vertex_map.entry(q).or_insert_with(|| {
                let new_idx = vertices.len();
                vertices.push(v);
                vertex_normals.push(Vec3 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                });
                new_idx
            });
            *tri_index = idx;
            // Accumulate normal for vertex
            let vn = &mut vertex_normals[idx];
            *vn = vn.add(&normal);
        }
        let mut attr = [0u8; 2];
        let _ = file.read_exact(&mut attr);
        indices.push(tri_indices);
    }

    // Normalize vertex normals
    for vn in &mut vertex_normals {
        *vn = vn.normalize();
    }

    // Determine bounding box for planar projection
    let mut min_z = f32::MAX;
    let mut max_z = f32::MIN;
    let mut min_x = f32::MAX;
    let mut max_x = f32::MIN;
    let mut min_y = f32::MAX;
    let mut max_y = f32::MIN;

    for v in &vertices {
        if v.z < min_z {
            min_z = v.z;
        }
        if v.z > max_z {
            max_z = v.z;
        }
        if v.x < min_x {
            min_x = v.x;
        }
        if v.x > max_x {
            max_x = v.x;
        }
        if v.y < min_y {
            min_y = v.y;
        }
        if v.y > max_y {
            max_y = v.y;
        }
    }

    let planar_axes = choose_planar_axes(min_x, max_x, min_y, max_y, min_z, max_z);
    let planar_projection =
        PlanarProjectionContext::new(planar_axes, min_x, max_x, min_y, max_y, min_z, max_z);
    if matches!(
        spec.projection,
        ProjectionType::Planar | ProjectionType::Auto
    ) {
        refine_planar_mesh(
            &mut vertices,
            &mut indices,
            &planar_projection,
            img_w,
            img_h,
        );
    }

    // Displace
    let mut new_vertices = Vec::with_capacity(vertices.len());
    let planar_normal_axis = planar_projection.normal_axis();
    let (_, planar_front_coord) = planar_projection.axis_bounds(planar_normal_axis);
    let planar_normal_span =
        planar_front_coord - planar_projection.axis_bounds(planar_normal_axis).0;
    let planar_front_tolerance = planar_normal_span.abs().max(1.0) * 1e-4;
    for i in 0..vertices.len() {
        let v = &vertices[i];

        let (u, mut v_tex) = match spec.projection {
            ProjectionType::Cylindrical => {
                let theta = v.y.atan2(v.x); // -PI to PI
                let u = (theta + std::f32::consts::PI) / (2.0 * std::f32::consts::PI);
                let z_span = max_z - min_z;
                let v_tex = if z_span > 1e-5 {
                    (v.z - min_z) / z_span
                } else {
                    0.0
                };
                (u, v_tex)
            }
            ProjectionType::Spherical => {
                let r = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
                let theta = v.y.atan2(v.x);
                let phi = if r > 1e-5 { (v.z / r).acos() } else { 0.0 }; // 0 to PI
                let u = (theta + std::f32::consts::PI) / (2.0 * std::f32::consts::PI);
                let v_tex = phi / std::f32::consts::PI;
                (u, v_tex)
            }
            ProjectionType::Planar | ProjectionType::Auto => planar_projection.uv(v),
        };

        // Clamp
        let u = u.clamp(0.0, 1.0);
        v_tex = v_tex.clamp(0.0, 1.0);
        // Images usually have 0,0 at top left.
        // For cylindrical/planar, we often want Z=min to be bottom, Z=max to be top.
        // So V should be 1.0 - v_tex.
        let v_tex = 1.0 - v_tex;

        let pixel = sample_luma_bilinear(&img, u, v_tex);

        let mut factor = pixel;
        if spec.invert {
            factor = 1.0 - factor;
        }

        let displacement = factor * spec.depth_mm as f32;
        let new_v = match spec.projection {
            ProjectionType::Planar | ProjectionType::Auto => {
                if (axis_value(v, planar_normal_axis) - planar_front_coord).abs()
                    <= planar_front_tolerance
                {
                    v.add(&axis_unit(planar_normal_axis).mul(displacement))
                } else {
                    *v
                }
            }
            ProjectionType::Cylindrical | ProjectionType::Spherical => {
                let n = &vertex_normals[i];
                v.add(&n.mul(displacement))
            }
        };
        new_vertices.push(new_v);
    }

    // Write output
    let mut out_file = File::create(output_stl)
        .map_err(|e| AppError::internal(format!("Failed to create output STL: {}", e)))?;

    out_file
        .write_all(&header)
        .map_err(|e| AppError::internal(format!("Failed to write STL header: {}", e)))?;
    let triangle_count = u32::try_from(indices.len())
        .map_err(|_| AppError::internal("Refined STL exceeded supported triangle count."))?;
    out_file
        .write_all(&triangle_count.to_le_bytes())
        .map_err(|e| AppError::internal(format!("Failed to write STL count: {}", e)))?;

    for tri in &indices {
        let v1 = &new_vertices[tri[0]];
        let v2 = &new_vertices[tri[1]];
        let v3 = &new_vertices[tri[2]];

        // Recompute face normal
        let e1 = v2.sub(v1);
        let e2 = v3.sub(v1);
        let n = e1.cross(&e2).normalize();

        write_vec3(&mut out_file, &n)
            .map_err(|e| AppError::internal(format!("Failed to write normal: {}", e)))?;
        write_vec3(&mut out_file, v1)
            .map_err(|e| AppError::internal(format!("Failed to write v1: {}", e)))?;
        write_vec3(&mut out_file, v2)
            .map_err(|e| AppError::internal(format!("Failed to write v2: {}", e)))?;
        write_vec3(&mut out_file, v3)
            .map_err(|e| AppError::internal(format!("Failed to write v3: {}", e)))?;

        out_file
            .write_all(&[0u8; 2])
            .map_err(|e| AppError::internal(format!("Failed to write attr: {}", e)))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{apply, choose_planar_axes, write_vec3, Axis, Vec3};
    use crate::contracts::{DisplacementSpec, ProjectionType};
    use image::{GrayImage, Luma};
    use std::fs::{self, File};
    use std::io::{Read, Write};
    use std::path::Path;
    use uuid::Uuid;

    fn write_test_stl(path: &Path, triangles: &[[Vec3; 3]]) {
        let mut file = File::create(path).unwrap();
        file.write_all(&[0u8; 80]).unwrap();
        file.write_all(&(triangles.len() as u32).to_le_bytes())
            .unwrap();

        for triangle in triangles {
            let e1 = triangle[1].sub(&triangle[0]);
            let e2 = triangle[2].sub(&triangle[0]);
            let normal = e1.cross(&e2).normalize();
            write_vec3(&mut file, &normal).unwrap();
            write_vec3(&mut file, &triangle[0]).unwrap();
            write_vec3(&mut file, &triangle[1]).unwrap();
            write_vec3(&mut file, &triangle[2]).unwrap();
            file.write_all(&[0u8; 2]).unwrap();
        }
    }

    fn read_stl_triangle_count(path: &Path) -> u32 {
        let mut file = File::open(path).unwrap();
        let mut header = [0u8; 80];
        file.read_exact(&mut header).unwrap();
        let mut count = [0u8; 4];
        file.read_exact(&mut count).unwrap();
        u32::from_le_bytes(count)
    }

    #[test]
    fn choose_planar_axes_uses_two_largest_bbox_dimensions() {
        let axes = choose_planar_axes(-128.0, 128.0, -1.5, 1.5, -100.0, 100.0);
        assert_eq!(axes, (Axis::X, Axis::Z));
    }

    #[test]
    fn choose_planar_axes_prefers_x_and_y_for_flat_top_down_shapes() {
        let axes = choose_planar_axes(-50.0, 50.0, -30.0, 30.0, -2.0, 2.0);
        assert_eq!(axes, (Axis::X, Axis::Y));
    }

    #[test]
    fn apply_planar_displacement_updates_binary_stl_triangle_count_after_refinement() {
        let root = std::env::temp_dir().join(format!("ecky-displacement-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();

        let input_stl = root.join("input.stl");
        let output_stl = root.join("output.stl");
        let image_path = root.join("height.png");

        write_test_stl(
            &input_stl,
            &[
                [
                    Vec3 {
                        x: -10.0,
                        y: -10.0,
                        z: 0.0,
                    },
                    Vec3 {
                        x: 10.0,
                        y: -10.0,
                        z: 0.0,
                    },
                    Vec3 {
                        x: 10.0,
                        y: 10.0,
                        z: 0.0,
                    },
                ],
                [
                    Vec3 {
                        x: -10.0,
                        y: -10.0,
                        z: 0.0,
                    },
                    Vec3 {
                        x: 10.0,
                        y: 10.0,
                        z: 0.0,
                    },
                    Vec3 {
                        x: -10.0,
                        y: 10.0,
                        z: 0.0,
                    },
                ],
            ],
        );

        let image = GrayImage::from_fn(64, 64, |x, y| {
            let sample = (((x + y) as f32 / 126.0) * 255.0).round() as u8;
            Luma([sample])
        });
        image.save(&image_path).unwrap();

        let spec = DisplacementSpec {
            image_param: "image".to_string(),
            projection: ProjectionType::Planar,
            depth_mm: 1.25,
            invert: false,
        };

        apply(&input_stl, image_path.to_str().unwrap(), &spec, &output_stl).unwrap();

        let triangle_count = read_stl_triangle_count(&output_stl);
        let file_size = fs::metadata(&output_stl).unwrap().len();

        assert!(
            triangle_count > 2,
            "planar displacement should refine the coarse input mesh"
        );
        assert_eq!(file_size, 84 + u64::from(triangle_count) * 50);

        fs::remove_dir_all(&root).unwrap();
    }
}
