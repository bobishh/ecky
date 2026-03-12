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
        for i in 0..3 {
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
            tri_indices[i] = idx;
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

    // Displace
    let mut new_vertices = Vec::with_capacity(vertices.len());
    for i in 0..vertices.len() {
        let v = &vertices[i];
        let n = &vertex_normals[i];

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
            ProjectionType::Planar => {
                let x_span = max_x - min_x;
                let y_span = max_y - min_y;
                let u = if x_span > 1e-5 {
                    (v.x - min_x) / x_span
                } else {
                    0.0
                };
                let v_tex = if y_span > 1e-5 {
                    (v.y - min_y) / y_span
                } else {
                    0.0
                };
                (u, v_tex)
            }
        };

        // Clamp
        let u = u.clamp(0.0, 1.0);
        v_tex = v_tex.clamp(0.0, 1.0);
        // Images usually have 0,0 at top left.
        // For cylindrical/planar, we often want Z=min to be bottom, Z=max to be top.
        // So V should be 1.0 - v_tex.
        let v_tex = 1.0 - v_tex;

        let px = (u * (img_w as f32 - 1.0)).round() as u32;
        let py = (v_tex * (img_h as f32 - 1.0)).round() as u32;

        let pixel = img.get_pixel(px.min(img_w - 1), py.min(img_h - 1)).0[0] as f32 / 255.0;

        let mut factor = pixel;
        if spec.invert {
            factor = 1.0 - factor;
        }

        let displacement = factor * spec.depth_mm as f32;
        let new_v = v.add(&n.mul(displacement));
        new_vertices.push(new_v);
    }

    // Write output
    let mut out_file = File::create(output_stl)
        .map_err(|e| AppError::internal(format!("Failed to create output STL: {}", e)))?;

    out_file
        .write_all(&header)
        .map_err(|e| AppError::internal(format!("Failed to write STL header: {}", e)))?;
    out_file
        .write_all(&num_triangles.to_le_bytes())
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
