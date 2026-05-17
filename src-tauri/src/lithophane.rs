use crate::contracts::{
    ExportArtifact, LithophaneAttachment, LithophaneAttachmentSource, LithophaneColorMode,
    LithophanePlacement, LithophaneRelief, LithophaneSide, ManifestBounds, OverflowMode,
    ProjectionType,
};
use crate::models::{AppError, AppResult};
use image::{GrayImage, RgbImage};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MAX_SAFE_PREVIEW_STL_BYTES: u64 = 64 * 1024 * 1024;
const PLANAR_SEGMENT_CEILING: f32 = 768.0;
const CYLINDRICAL_SEGMENT_CEILING: f32 = 192.0;
const SPHERICAL_SEGMENT_CEILING: f32 = 160.0;

#[derive(Clone, Copy, Debug)]
struct Vec3 {
    x: f32,
    y: f32,
    z: f32,
}

impl Vec3 {
    fn add(self, other: Vec3) -> Vec3 {
        Vec3 {
            x: self.x + other.x,
            y: self.y + other.y,
            z: self.z + other.z,
        }
    }

    fn sub(self, other: Vec3) -> Vec3 {
        Vec3 {
            x: self.x - other.x,
            y: self.y - other.y,
            z: self.z - other.z,
        }
    }

    fn mul(self, scalar: f32) -> Vec3 {
        Vec3 {
            x: self.x * scalar,
            y: self.y * scalar,
            z: self.z * scalar,
        }
    }

    fn dot(self, other: Vec3) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    fn cross(self, other: Vec3) -> Vec3 {
        Vec3 {
            x: self.y * other.z - self.z * other.y,
            y: self.z * other.x - self.x * other.z,
            z: self.x * other.y - self.y * other.x,
        }
    }

    fn normalize(self) -> Vec3 {
        let len = (self.x * self.x + self.y * self.y + self.z * self.z).sqrt();
        if len > 1e-6 {
            self.mul(1.0 / len)
        } else {
            self
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Bounds {
    min: Vec3,
    max: Vec3,
}

impl Bounds {
    fn from_vertices(vertices: &[Vec3]) -> Self {
        let mut min = Vec3 {
            x: f32::MAX,
            y: f32::MAX,
            z: f32::MAX,
        };
        let mut max = Vec3 {
            x: f32::MIN,
            y: f32::MIN,
            z: f32::MIN,
        };
        for vertex in vertices {
            min.x = min.x.min(vertex.x);
            min.y = min.y.min(vertex.y);
            min.z = min.z.min(vertex.z);
            max.x = max.x.max(vertex.x);
            max.y = max.y.max(vertex.y);
            max.z = max.z.max(vertex.z);
        }
        Self { min, max }
    }

    fn center(self) -> Vec3 {
        Vec3 {
            x: (self.min.x + self.max.x) * 0.5,
            y: (self.min.y + self.max.y) * 0.5,
            z: (self.min.z + self.max.z) * 0.5,
        }
    }
}

impl From<&ManifestBounds> for Bounds {
    fn from(value: &ManifestBounds) -> Self {
        Self {
            min: Vec3 {
                x: value.x_min as f32,
                y: value.y_min as f32,
                z: value.z_min as f32,
            },
            max: Vec3 {
                x: value.x_max as f32,
                y: value.y_max as f32,
                z: value.z_max as f32,
            },
        }
    }
}

#[derive(Clone, Debug)]
struct Mesh {
    header: [u8; 80],
    vertices: Vec<Vec3>,
    vertex_normals: Vec<Vec3>,
    indices: Vec<[usize; 3]>,
}

#[derive(Clone, Debug)]
pub struct ResolvedLithophaneAttachment {
    pub id: String,
    pub image_path: String,
    pub target_bounds: Option<ManifestBounds>,
    pub placement: LithophanePlacement,
    pub relief: LithophaneRelief,
    pub color_mode: LithophaneColorMode,
    pub channel_thickness_mm: f64,
}

#[derive(Clone, Copy, Debug)]
struct PatchFrame {
    center: Vec3,
    u_axis: Vec3,
    v_axis: Vec3,
    normal_axis: Vec3,
    half_width: f32,
    half_height: f32,
    rotation_rad: f32,
    normal_span: f32,
    overflow_mode: OverflowMode,
    bleed_margin_mm: f32,
}

impl PatchFrame {
    fn from_bounds(bounds: Bounds, placement: &LithophanePlacement) -> Self {
        let center = bounds.center();
        let (mut origin, u_axis, v_axis, normal_axis, default_width, default_height, normal_span) =
            match placement.side {
                LithophaneSide::Front => (
                    Vec3 {
                        x: center.x,
                        y: bounds.max.y,
                        z: center.z,
                    },
                    Vec3 {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    Vec3 {
                        x: 0.0,
                        y: 0.0,
                        z: 1.0,
                    },
                    Vec3 {
                        x: 0.0,
                        y: 1.0,
                        z: 0.0,
                    },
                    (bounds.max.x - bounds.min.x).abs(),
                    (bounds.max.z - bounds.min.z).abs(),
                    (bounds.max.y - bounds.min.y).abs(),
                ),
                LithophaneSide::Back => (
                    Vec3 {
                        x: center.x,
                        y: bounds.min.y,
                        z: center.z,
                    },
                    Vec3 {
                        x: -1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    Vec3 {
                        x: 0.0,
                        y: 0.0,
                        z: 1.0,
                    },
                    Vec3 {
                        x: 0.0,
                        y: -1.0,
                        z: 0.0,
                    },
                    (bounds.max.x - bounds.min.x).abs(),
                    (bounds.max.z - bounds.min.z).abs(),
                    (bounds.max.y - bounds.min.y).abs(),
                ),
                LithophaneSide::Left => (
                    Vec3 {
                        x: bounds.min.x,
                        y: center.y,
                        z: center.z,
                    },
                    Vec3 {
                        x: 0.0,
                        y: 1.0,
                        z: 0.0,
                    },
                    Vec3 {
                        x: 0.0,
                        y: 0.0,
                        z: 1.0,
                    },
                    Vec3 {
                        x: -1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    (bounds.max.y - bounds.min.y).abs(),
                    (bounds.max.z - bounds.min.z).abs(),
                    (bounds.max.x - bounds.min.x).abs(),
                ),
                LithophaneSide::Right => (
                    Vec3 {
                        x: bounds.max.x,
                        y: center.y,
                        z: center.z,
                    },
                    Vec3 {
                        x: 0.0,
                        y: -1.0,
                        z: 0.0,
                    },
                    Vec3 {
                        x: 0.0,
                        y: 0.0,
                        z: 1.0,
                    },
                    Vec3 {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    (bounds.max.y - bounds.min.y).abs(),
                    (bounds.max.z - bounds.min.z).abs(),
                    (bounds.max.x - bounds.min.x).abs(),
                ),
                LithophaneSide::Top => (
                    Vec3 {
                        x: center.x,
                        y: center.y,
                        z: bounds.max.z,
                    },
                    Vec3 {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    Vec3 {
                        x: 0.0,
                        y: 1.0,
                        z: 0.0,
                    },
                    Vec3 {
                        x: 0.0,
                        y: 0.0,
                        z: 1.0,
                    },
                    (bounds.max.x - bounds.min.x).abs(),
                    (bounds.max.y - bounds.min.y).abs(),
                    (bounds.max.z - bounds.min.z).abs(),
                ),
                LithophaneSide::Bottom => (
                    Vec3 {
                        x: center.x,
                        y: center.y,
                        z: bounds.min.z,
                    },
                    Vec3 {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    Vec3 {
                        x: 0.0,
                        y: -1.0,
                        z: 0.0,
                    },
                    Vec3 {
                        x: 0.0,
                        y: 0.0,
                        z: -1.0,
                    },
                    (bounds.max.x - bounds.min.x).abs(),
                    (bounds.max.y - bounds.min.y).abs(),
                    (bounds.max.z - bounds.min.z).abs(),
                ),
            };

        let width = if placement.width_mm > 0.0 {
            placement.width_mm as f32
        } else {
            default_width.max(1.0)
        };
        let height = if placement.height_mm > 0.0 {
            placement.height_mm as f32
        } else {
            default_height.max(1.0)
        };

        origin = origin
            .add(u_axis.mul(placement.offset_x_mm as f32))
            .add(v_axis.mul(placement.offset_y_mm as f32));

        Self {
            center: origin,
            u_axis,
            v_axis,
            normal_axis,
            half_width: width * 0.5,
            half_height: height * 0.5,
            rotation_rad: (placement.rotation_deg as f32).to_radians(),
            normal_span: normal_span.max(1.0),
            overflow_mode: placement.overflow_mode,
            bleed_margin_mm: placement.bleed_margin_mm.max(0.0) as f32,
        }
    }

    fn width(self) -> f32 {
        self.half_width * 2.0
    }

    fn height(self) -> f32 {
        self.half_height * 2.0
    }

    fn raw_coords(self, point: Vec3) -> (f32, f32, f32) {
        let relative = point.sub(self.center);
        let du = relative.dot(self.u_axis);
        let dv = relative.dot(self.v_axis);
        let dn = relative.dot(self.normal_axis);
        let cos = self.rotation_rad.cos();
        let sin = self.rotation_rad.sin();
        let rotated_u = du * cos + dv * sin;
        let rotated_v = -du * sin + dv * cos;
        (
            (rotated_u + self.half_width) / self.width(),
            (rotated_v + self.half_height) / self.height(),
            dn,
        )
    }

    fn point(self, u: f32, v: f32, normal_offset: f32) -> Vec3 {
        let local_u = -self.half_width + u * self.width();
        let local_v = -self.half_height + v * self.height();
        let cos = self.rotation_rad.cos();
        let sin = self.rotation_rad.sin();
        let unrotated_u = local_u * cos - local_v * sin;
        let unrotated_v = local_u * sin + local_v * cos;
        self.center
            .add(self.u_axis.mul(unrotated_u))
            .add(self.v_axis.mul(unrotated_v))
            .add(self.normal_axis.mul(normal_offset))
    }

    fn surface_tolerance(self) -> f32 {
        (self.normal_span * 0.01).max(0.25)
    }

    fn curved_side_cutoff(self) -> f32 {
        -(self.normal_span * 0.35).max(0.75)
    }

    fn refinement_margin(self) -> f32 {
        if matches!(self.overflow_mode, OverflowMode::Bleed)
            && self.width() > 0.0
            && self.height() > 0.0
        {
            (self.bleed_margin_mm / self.width())
                .max(self.bleed_margin_mm / self.height())
                .max(0.05)
        } else {
            0.05
        }
    }
}

fn quantize(v: Vec3) -> (i32, i32, i32) {
    (
        (v.x * 1000.0).round() as i32,
        (v.y * 1000.0).round() as i32,
        (v.z * 1000.0).round() as i32,
    )
}

fn edge_key(a: usize, b: usize) -> (usize, usize) {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}

fn midpoint(left: Vec3, right: Vec3) -> Vec3 {
    Vec3 {
        x: (left.x + right.x) * 0.5,
        y: (left.y + right.y) * 0.5,
        z: (left.z + right.z) * 0.5,
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

fn write_f32<W: Write>(writer: &mut W, value: f32) -> std::io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn write_vec3<W: Write>(writer: &mut W, value: Vec3) -> std::io::Result<()> {
    write_f32(writer, value.x)?;
    write_f32(writer, value.y)?;
    write_f32(writer, value.z)
}

fn read_mesh(path: &Path) -> AppResult<Mesh> {
    let mut file = File::open(path)
        .map_err(|e| AppError::internal(format!("Failed to open input STL: {}", e)))?;

    let mut header = [0u8; 80];
    file.read_exact(&mut header)
        .map_err(|e| AppError::internal(format!("Failed to read STL header: {}", e)))?;

    let mut count_buf = [0u8; 4];
    file.read_exact(&mut count_buf)
        .map_err(|e| AppError::internal(format!("Failed to read STL triangle count: {}", e)))?;
    let triangle_count = u32::from_le_bytes(count_buf);

    let mut vertices = Vec::new();
    let mut vertex_normals = Vec::new();
    let mut vertex_map: HashMap<(i32, i32, i32), usize> = HashMap::new();
    let mut indices = Vec::with_capacity(triangle_count as usize);

    for _ in 0..triangle_count {
        let normal = read_vec3(&mut file).unwrap_or(Vec3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        });
        let mut tri = [0usize; 3];
        for slot in &mut tri {
            let vertex = read_vec3(&mut file)
                .map_err(|e| AppError::internal(format!("Failed to read STL vertex: {}", e)))?;
            let key = quantize(vertex);
            let index = *vertex_map.entry(key).or_insert_with(|| {
                let next = vertices.len();
                vertices.push(vertex);
                vertex_normals.push(Vec3 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                });
                next
            });
            *slot = index;
            vertex_normals[index] = vertex_normals[index].add(normal);
        }
        let mut attr = [0u8; 2];
        let _ = file.read_exact(&mut attr);
        indices.push(tri);
    }

    let mut mesh = Mesh {
        header,
        vertices,
        vertex_normals,
        indices,
    };
    recompute_normals(&mut mesh);
    Ok(mesh)
}

fn write_mesh(path: &Path, mesh: &Mesh) -> AppResult<()> {
    let mut file = File::create(path)
        .map_err(|e| AppError::internal(format!("Failed to create output STL: {}", e)))?;
    file.write_all(&mesh.header)
        .map_err(|e| AppError::internal(format!("Failed to write STL header: {}", e)))?;
    let triangle_count = u32::try_from(mesh.indices.len())
        .map_err(|_| AppError::internal("Refined STL exceeded supported triangle count."))?;
    file.write_all(&triangle_count.to_le_bytes())
        .map_err(|e| AppError::internal(format!("Failed to write STL triangle count: {}", e)))?;

    for tri in &mesh.indices {
        let v1 = mesh.vertices[tri[0]];
        let v2 = mesh.vertices[tri[1]];
        let v3 = mesh.vertices[tri[2]];
        let normal = v2.sub(v1).cross(v3.sub(v1)).normalize();
        write_vec3(&mut file, normal)
            .and_then(|_| write_vec3(&mut file, v1))
            .and_then(|_| write_vec3(&mut file, v2))
            .and_then(|_| write_vec3(&mut file, v3))
            .and_then(|_| file.write_all(&[0u8; 2]))
            .map_err(|e| AppError::internal(format!("Failed to write output STL: {}", e)))?;
    }

    Ok(())
}

fn recompute_normals(mesh: &mut Mesh) {
    mesh.vertex_normals = vec![
        Vec3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        mesh.vertices.len()
    ];
    for tri in &mesh.indices {
        let a = mesh.vertices[tri[0]];
        let b = mesh.vertices[tri[1]];
        let c = mesh.vertices[tri[2]];
        let normal = b.sub(a).cross(c.sub(a)).normalize();
        mesh.vertex_normals[tri[0]] = mesh.vertex_normals[tri[0]].add(normal);
        mesh.vertex_normals[tri[1]] = mesh.vertex_normals[tri[1]].add(normal);
        mesh.vertex_normals[tri[2]] = mesh.vertex_normals[tri[2]].add(normal);
    }
    for normal in &mut mesh.vertex_normals {
        *normal = normal.normalize();
    }
}

fn bilinear_gray(image: &GrayImage, u: f32, v: f32) -> f32 {
    let width = image.width().max(1);
    let height = image.height().max(1);
    let x = u.clamp(0.0, 1.0) * (width as f32 - 1.0);
    let y = v.clamp(0.0, 1.0) * (height as f32 - 1.0);
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(width - 1);
    let y1 = (y0 + 1).min(height - 1);
    let tx = x - x0 as f32;
    let ty = y - y0 as f32;
    let p00 = image.get_pixel(x0, y0).0[0] as f32 / 255.0;
    let p10 = image.get_pixel(x1, y0).0[0] as f32 / 255.0;
    let p01 = image.get_pixel(x0, y1).0[0] as f32 / 255.0;
    let p11 = image.get_pixel(x1, y1).0[0] as f32 / 255.0;
    let top = p00 + (p10 - p00) * tx;
    let bottom = p01 + (p11 - p01) * tx;
    top + (bottom - top) * ty
}

fn bilinear_rgb(image: &RgbImage, u: f32, v: f32) -> [f32; 3] {
    let width = image.width().max(1);
    let height = image.height().max(1);
    let x = u.clamp(0.0, 1.0) * (width as f32 - 1.0);
    let y = v.clamp(0.0, 1.0) * (height as f32 - 1.0);
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(width - 1);
    let y1 = (y0 + 1).min(height - 1);
    let tx = x - x0 as f32;
    let ty = y - y0 as f32;

    let mix = |channel: usize| -> f32 {
        let p00 = image.get_pixel(x0, y0).0[channel] as f32 / 255.0;
        let p10 = image.get_pixel(x1, y0).0[channel] as f32 / 255.0;
        let p01 = image.get_pixel(x0, y1).0[channel] as f32 / 255.0;
        let p11 = image.get_pixel(x1, y1).0[channel] as f32 / 255.0;
        let top = p00 + (p10 - p00) * tx;
        let bottom = p01 + (p11 - p01) * tx;
        top + (bottom - top) * ty
    };

    [mix(0), mix(1), mix(2)]
}

fn rgb_to_cmyk(rgb: [f32; 3]) -> [f32; 4] {
    let c = 1.0 - rgb[0];
    let m = 1.0 - rgb[1];
    let y = 1.0 - rgb[2];
    let k = c.min(m).min(y);
    if k >= 0.999 {
        [0.0, 0.0, 0.0, 1.0]
    } else {
        let denom = 1.0 - k;
        [(c - k) / denom, (m - k) / denom, (y - k) / denom, k]
    }
}

fn compute_fit_uv(
    frame: PatchFrame,
    image_width: u32,
    image_height: u32,
    raw_u: f32,
    raw_v: f32,
) -> Option<(f32, f32)> {
    if !(0.0..=1.0).contains(&raw_u) || !(0.0..=1.0).contains(&raw_v) {
        return None;
    }

    let mut patch_u = raw_u;
    let mut patch_v = raw_v;
    if matches!(frame.overflow_mode, OverflowMode::Bleed) && frame.bleed_margin_mm > 0.0 {
        let expanded_width = frame.width() + frame.bleed_margin_mm * 2.0;
        let expanded_height = frame.height() + frame.bleed_margin_mm * 2.0;
        patch_u = (raw_u * frame.width() + frame.bleed_margin_mm) / expanded_width;
        patch_v = (raw_v * frame.height() + frame.bleed_margin_mm) / expanded_height;
    }

    let patch_aspect = (frame.width() / frame.height()).max(1e-6);
    let image_aspect = (image_width as f32 / image_height.max(1) as f32).max(1e-6);

    match frame.overflow_mode {
        OverflowMode::Contain | OverflowMode::Clamp => {
            if patch_aspect >= image_aspect {
                let display_width = image_aspect / patch_aspect;
                let x_min = (1.0 - display_width) * 0.5;
                if patch_u < x_min || patch_u > x_min + display_width {
                    return None;
                }
                Some(((patch_u - x_min) / display_width, patch_v))
            } else {
                let display_height = patch_aspect / image_aspect;
                let y_min = (1.0 - display_height) * 0.5;
                if patch_v < y_min || patch_v > y_min + display_height {
                    return None;
                }
                Some((patch_u, (patch_v - y_min) / display_height))
            }
        }
        OverflowMode::Cover | OverflowMode::Bleed => {
            if patch_aspect >= image_aspect {
                let crop_height = image_aspect / patch_aspect;
                let y_min = (1.0 - crop_height) * 0.5;
                Some((patch_u, y_min + patch_v * crop_height))
            } else {
                let crop_width = patch_aspect / image_aspect;
                let x_min = (1.0 - crop_width) * 0.5;
                Some((x_min + patch_u * crop_width, patch_v))
            }
        }
    }
}

fn projection_segment_density(projection: ProjectionType) -> f32 {
    match projection {
        ProjectionType::Planar => 8.0,
        ProjectionType::Cylindrical => 2.5,
        ProjectionType::Spherical => 2.0,
        ProjectionType::Auto => 4.0,
    }
}

fn projection_segment_floor(projection: ProjectionType) -> f32 {
    match projection {
        ProjectionType::Planar => 96.0,
        ProjectionType::Cylindrical | ProjectionType::Spherical => 64.0,
        ProjectionType::Auto => 80.0,
    }
}

fn projection_segment_ceiling(projection: ProjectionType) -> f32 {
    match projection {
        ProjectionType::Planar => PLANAR_SEGMENT_CEILING,
        ProjectionType::Cylindrical => CYLINDRICAL_SEGMENT_CEILING,
        ProjectionType::Spherical => SPHERICAL_SEGMENT_CEILING,
        ProjectionType::Auto => CYLINDRICAL_SEGMENT_CEILING,
    }
}

fn sample_target_segments(
    frame: PatchFrame,
    projection: ProjectionType,
    image_width: u32,
    image_height: u32,
) -> f32 {
    let image_longest = image_width.max(image_height) as f32;
    let longest_mm = frame.width().max(frame.height());
    let segment_floor = projection_segment_floor(projection);
    image_longest
        .min((longest_mm * projection_segment_density(projection)).max(segment_floor))
        .clamp(segment_floor, projection_segment_ceiling(projection))
}

fn triangle_near_patch(frame: PatchFrame, tri: &[usize; 3], vertices: &[Vec3]) -> bool {
    let margin = frame.refinement_margin();
    let coords = tri.map(|index| frame.raw_coords(vertices[index]));
    let min_u = coords.iter().map(|(u, _, _)| *u).fold(f32::MAX, f32::min);
    let max_u = coords.iter().map(|(u, _, _)| *u).fold(f32::MIN, f32::max);
    let min_v = coords.iter().map(|(_, v, _)| *v).fold(f32::MAX, f32::min);
    let max_v = coords.iter().map(|(_, v, _)| *v).fold(f32::MIN, f32::max);
    max_u >= -margin && min_u <= 1.0 + margin && max_v >= -margin && min_v <= 1.0 + margin
}

fn max_patch_edge_span(frame: PatchFrame, tri: &[usize; 3], vertices: &[Vec3]) -> f32 {
    let coords = tri.map(|index| frame.raw_coords(vertices[index]));
    [
        (coords[0], coords[1]),
        (coords[1], coords[2]),
        (coords[2], coords[0]),
    ]
    .into_iter()
    .map(|((u0, v0, _), (u1, v1, _))| (u0 - u1).abs().max((v0 - v1).abs()))
    .fold(0.0, f32::max)
}

fn refine_mesh_for_attachment(
    mesh: &mut Mesh,
    frame: PatchFrame,
    projection: ProjectionType,
    image_width: u32,
    image_height: u32,
) {
    let uv_edge_limit = 1.0 / sample_target_segments(frame, projection, image_width, image_height);
    for _ in 0..6 {
        let mut midpoint_cache = HashMap::new();
        let mut next_indices = Vec::with_capacity(mesh.indices.len());
        let mut split_count = 0usize;

        for tri in mesh.indices.iter() {
            if !triangle_near_patch(frame, tri, &mesh.vertices) {
                next_indices.push(*tri);
                continue;
            }
            if max_patch_edge_span(frame, tri, &mesh.vertices) <= uv_edge_limit {
                next_indices.push(*tri);
                continue;
            }

            split_count += 1;
            let midpoint_index = |left: usize,
                                  right: usize,
                                  vertices: &mut Vec<Vec3>,
                                  cache: &mut HashMap<(usize, usize), usize>|
             -> usize {
                let key = edge_key(left, right);
                if let Some(existing) = cache.get(&key) {
                    return *existing;
                }
                let idx = vertices.len();
                vertices.push(midpoint(vertices[left], vertices[right]));
                cache.insert(key, idx);
                idx
            };

            let a = tri[0];
            let b = tri[1];
            let c = tri[2];
            let ab = midpoint_index(a, b, &mut mesh.vertices, &mut midpoint_cache);
            let bc = midpoint_index(b, c, &mut mesh.vertices, &mut midpoint_cache);
            let ca = midpoint_index(c, a, &mut mesh.vertices, &mut midpoint_cache);
            next_indices.push([a, ab, ca]);
            next_indices.push([ab, b, bc]);
            next_indices.push([ca, bc, c]);
            next_indices.push([ab, bc, ca]);
        }

        mesh.indices = next_indices;
        if split_count == 0 {
            break;
        }
    }
    recompute_normals(mesh);
}

fn resolve_projection(placement: &LithophanePlacement, frame: PatchFrame) -> ProjectionType {
    match placement.projection {
        ProjectionType::Auto => {
            if frame.normal_span <= 2.0 {
                ProjectionType::Planar
            } else {
                ProjectionType::Cylindrical
            }
        }
        projection => projection,
    }
}

fn apply_attachment_to_mesh(
    mesh: &mut Mesh,
    frame: PatchFrame,
    attachment: &ResolvedLithophaneAttachment,
    image: &GrayImage,
) {
    let projection = resolve_projection(&attachment.placement, frame);
    for (index, vertex) in mesh.vertices.clone().into_iter().enumerate() {
        let (raw_u, raw_v, dn) = frame.raw_coords(vertex);
        let Some((image_u, image_v)) =
            compute_fit_uv(frame, image.width(), image.height(), raw_u, 1.0 - raw_v)
        else {
            continue;
        };
        let luma = bilinear_gray(image, image_u, image_v);
        let factor = if attachment.relief.invert {
            1.0 - luma
        } else {
            luma
        };
        let displacement = factor * attachment.relief.depth_mm as f32;
        let next_vertex = match projection {
            ProjectionType::Planar => {
                if dn.abs() <= frame.surface_tolerance() {
                    vertex.add(frame.normal_axis.mul(displacement))
                } else {
                    vertex
                }
            }
            ProjectionType::Auto | ProjectionType::Cylindrical | ProjectionType::Spherical => {
                if dn < frame.curved_side_cutoff() {
                    vertex
                } else {
                    vertex.add(mesh.vertex_normals[index].mul(displacement))
                }
            }
        };
        mesh.vertices[index] = next_vertex;
    }
    recompute_normals(mesh);
}

fn write_triangle_mesh(path: &Path, triangles: &[[Vec3; 3]]) -> AppResult<()> {
    let mut file = File::create(path).map_err(|e| {
        AppError::internal(format!("Failed to create STL '{}': {}", path.display(), e))
    })?;
    file.write_all(&[0u8; 80])
        .map_err(|e| AppError::internal(format!("Failed to write STL header: {}", e)))?;
    file.write_all(&(triangles.len() as u32).to_le_bytes())
        .map_err(|e| AppError::internal(format!("Failed to write STL triangle count: {}", e)))?;
    for triangle in triangles {
        let normal = triangle[1]
            .sub(triangle[0])
            .cross(triangle[2].sub(triangle[0]))
            .normalize();
        write_vec3(&mut file, normal)
            .and_then(|_| write_vec3(&mut file, triangle[0]))
            .and_then(|_| write_vec3(&mut file, triangle[1]))
            .and_then(|_| write_vec3(&mut file, triangle[2]))
            .and_then(|_| file.write_all(&[0u8; 2]))
            .map_err(|e| {
                AppError::internal(format!("Failed to write STL '{}': {}", path.display(), e))
            })?;
    }
    Ok(())
}

fn build_channel_overlay_mesh(
    frame: PatchFrame,
    projection: ProjectionType,
    image_rgb: &RgbImage,
    image_luma: &GrayImage,
    attachment: &ResolvedLithophaneAttachment,
    channel_index: usize,
) -> Option<Vec<[Vec3; 3]>> {
    let target = sample_target_segments(frame, projection, image_rgb.width(), image_rgb.height())
        .clamp(48.0, 384.0);
    let aspect = (frame.width() / frame.height()).max(1e-6);
    let segments_u = target as usize;
    let segments_v = ((target / aspect).round() as usize).clamp(16, 384);

    let mut top = vec![
        Vec3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        (segments_u + 1) * (segments_v + 1)
    ];
    let mut bottom = top.clone();
    let mut max_channel = 0.0f32;

    for iy in 0..=segments_v {
        for ix in 0..=segments_u {
            let u = ix as f32 / segments_u.max(1) as f32;
            let v = iy as f32 / segments_v.max(1) as f32;
            let Some((image_u, image_v)) =
                compute_fit_uv(frame, image_rgb.width(), image_rgb.height(), u, 1.0 - v)
            else {
                let point = frame.point(u, v, 0.0);
                let idx = iy * (segments_u + 1) + ix;
                top[idx] = point;
                bottom[idx] = point;
                continue;
            };

            let luma = bilinear_gray(image_luma, image_u, image_v);
            let relief = if attachment.relief.invert {
                1.0 - luma
            } else {
                luma
            };
            let channel = rgb_to_cmyk(bilinear_rgb(image_rgb, image_u, image_v))[channel_index];
            max_channel = max_channel.max(channel);
            let base_offset = relief * attachment.relief.depth_mm as f32;
            let channel_offset = channel * attachment.channel_thickness_mm as f32;
            let idx = iy * (segments_u + 1) + ix;
            bottom[idx] = frame.point(u, v, base_offset);
            top[idx] = frame.point(u, v, base_offset + channel_offset);
        }
    }

    if max_channel < 0.001 {
        return None;
    }

    let mut triangles = Vec::new();
    let idx = |x: usize, y: usize| -> usize { y * (segments_u + 1) + x };

    for y in 0..segments_v {
        for x in 0..segments_u {
            let a = idx(x, y);
            let b = idx(x + 1, y);
            let c = idx(x + 1, y + 1);
            let d = idx(x, y + 1);
            triangles.push([top[a], top[b], top[c]]);
            triangles.push([top[a], top[c], top[d]]);
            triangles.push([bottom[a], bottom[c], bottom[b]]);
            triangles.push([bottom[a], bottom[d], bottom[c]]);
        }
    }

    for x in 0..segments_u {
        let a = idx(x, 0);
        let b = idx(x + 1, 0);
        triangles.push([bottom[a], bottom[b], top[b]]);
        triangles.push([bottom[a], top[b], top[a]]);

        let c = idx(x, segments_v);
        let d = idx(x + 1, segments_v);
        triangles.push([bottom[c], top[d], bottom[d]]);
        triangles.push([bottom[c], top[c], top[d]]);
    }

    for y in 0..segments_v {
        let a = idx(0, y);
        let b = idx(0, y + 1);
        triangles.push([bottom[a], top[b], bottom[b]]);
        triangles.push([bottom[a], top[a], top[b]]);

        let c = idx(segments_u, y);
        let d = idx(segments_u, y + 1);
        triangles.push([bottom[c], bottom[d], top[d]]);
        triangles.push([bottom[c], top[d], top[c]]);
    }

    Some(triangles)
}

fn read_binary_stl_triangles(path: &Path) -> AppResult<Vec<[Vec3; 3]>> {
    let mut file = File::open(path).map_err(|e| {
        AppError::internal(format!("Failed to open STL '{}': {}", path.display(), e))
    })?;
    let mut header = [0u8; 80];
    file.read_exact(&mut header)
        .map_err(|e| AppError::internal(format!("Failed to read STL header: {}", e)))?;
    let mut count = [0u8; 4];
    file.read_exact(&mut count)
        .map_err(|e| AppError::internal(format!("Failed to read STL triangle count: {}", e)))?;
    let triangle_count = u32::from_le_bytes(count);
    let mut triangles = Vec::with_capacity(triangle_count as usize);
    for _ in 0..triangle_count {
        let _normal = read_vec3(&mut file)
            .map_err(|e| AppError::internal(format!("Failed to read STL normal: {}", e)))?;
        let a = read_vec3(&mut file)
            .map_err(|e| AppError::internal(format!("Failed to read STL vertex: {}", e)))?;
        let b = read_vec3(&mut file)
            .map_err(|e| AppError::internal(format!("Failed to read STL vertex: {}", e)))?;
        let c = read_vec3(&mut file)
            .map_err(|e| AppError::internal(format!("Failed to read STL vertex: {}", e)))?;
        let mut attr = [0u8; 2];
        let _ = file.read_exact(&mut attr);
        triangles.push([a, b, c]);
    }
    Ok(triangles)
}

#[derive(Clone)]
struct ThreeMfObject {
    id: u32,
    name: String,
    color_index: usize,
    triangles: Vec<[Vec3; 3]>,
}

fn indexed_3mf_mesh(triangles: &[[Vec3; 3]]) -> (Vec<Vec3>, Vec<[usize; 3]>) {
    let mut vertices = Vec::<Vec3>::new();
    let mut vertex_map = HashMap::<(i32, i32, i32), usize>::new();
    let mut indexed_triangles = Vec::<[usize; 3]>::with_capacity(triangles.len());

    for triangle in triangles {
        let mut indexed = [0usize; 3];
        for (slot, vertex) in triangle.iter().copied().enumerate() {
            let key = quantize(vertex);
            let index = *vertex_map.entry(key).or_insert_with(|| {
                let next = vertices.len();
                vertices.push(vertex);
                next
            });
            indexed[slot] = index;
        }
        indexed_triangles.push(indexed);
    }

    (vertices, indexed_triangles)
}

fn write_3mf_package(path: &Path, objects: &[ThreeMfObject]) -> AppResult<()> {
    let file = File::create(path).map_err(|e| {
        AppError::internal(format!("Failed to create 3MF '{}': {}", path.display(), e))
    })?;
    let mut zip = zip::ZipWriter::new(file);
    let options =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);

    zip.start_file("[Content_Types].xml", options)
        .map_err(|e| AppError::internal(format!("Failed to write 3MF content types: {}", e)))?;
    zip.write_all(br#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/></Types>"#)
        .map_err(|e| AppError::internal(format!("Failed to write 3MF content types: {}", e)))?;

    zip.add_directory("_rels/", options)
        .map_err(|e| AppError::internal(format!("Failed to add 3MF rels dir: {}", e)))?;
    zip.start_file("_rels/.rels", options)
        .map_err(|e| AppError::internal(format!("Failed to write 3MF rels: {}", e)))?;
    zip.write_all(br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Target="/3D/3dmodel.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/></Relationships>"#)
        .map_err(|e| AppError::internal(format!("Failed to write 3MF rels: {}", e)))?;

    zip.add_directory("3D/", options)
        .map_err(|e| AppError::internal(format!("Failed to add 3MF 3D dir: {}", e)))?;
    zip.start_file("3D/3dmodel.model", options)
        .map_err(|e| AppError::internal(format!("Failed to write 3MF model: {}", e)))?;

    let mut xml = String::new();
    xml.push_str(
        r##"<?xml version="1.0" encoding="UTF-8"?><model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02"><resources><basematerials id="1"><base name="Base" displaycolor="#D8D8D8FF"/><base name="Cyan" displaycolor="#00FFFFFF"/><base name="Magenta" displaycolor="#FF00FFFF"/><base name="Yellow" displaycolor="#FFFF00FF"/><base name="Black" displaycolor="#000000FF"/></basematerials>"##,
    );
    for object in objects {
        let (vertices, triangles) = indexed_3mf_mesh(&object.triangles);
        let _ = write!(
            xml,
            r#"<object id="{}" type="model" pid="1" pindex="{}" name="{}"><mesh><vertices>"#,
            object.id, object.color_index, object.name
        );
        for vertex in vertices {
            let _ = write!(
                xml,
                r#"<vertex x="{:.5}" y="{:.5}" z="{:.5}"/>"#,
                vertex.x, vertex.y, vertex.z
            );
        }
        xml.push_str("</vertices><triangles>");
        for triangle in triangles {
            let _ = write!(
                xml,
                r#"<triangle v1="{}" v2="{}" v3="{}"/>"#,
                triangle[0], triangle[1], triangle[2]
            );
        }
        xml.push_str("</triangles></mesh></object>");
    }
    xml.push_str("</resources><build>");
    for object in objects {
        let _ = write!(xml, r#"<item objectid="{}"/>"#, object.id);
    }
    xml.push_str("</build></model>");
    zip.write_all(xml.as_bytes())
        .map_err(|e| AppError::internal(format!("Failed to write 3MF model XML: {}", e)))?;
    zip.finish()
        .map_err(|e| AppError::internal(format!("Failed to finalize 3MF: {}", e)))?;
    Ok(())
}

fn color_channel_label(channel_index: usize) -> &'static str {
    match channel_index {
        0 => "C",
        1 => "M",
        2 => "Y",
        _ => "K",
    }
}

fn generate_cmyk_export_artifacts(
    export_dir: &Path,
    output_stl: &Path,
    attachment: &ResolvedLithophaneAttachment,
    frame: PatchFrame,
    projection: ProjectionType,
    image_rgb: &RgbImage,
    image_luma: &GrayImage,
) -> AppResult<Vec<ExportArtifact>> {
    let mut objects = Vec::new();
    objects.push(ThreeMfObject {
        id: 1,
        name: "Base Relief".to_string(),
        color_index: 0,
        triangles: read_binary_stl_triangles(output_stl)?,
    });

    let mut artifacts = Vec::new();
    for channel_index in 0..4 {
        let Some(mesh) = build_channel_overlay_mesh(
            frame,
            projection,
            image_rgb,
            image_luma,
            attachment,
            channel_index,
        ) else {
            continue;
        };
        let channel_label = color_channel_label(channel_index);
        let stl_path = export_dir.join(format!("{}-{}.stl", attachment.id, channel_label));
        write_triangle_mesh(&stl_path, &mesh)?;
        artifacts.push(ExportArtifact {
            label: format!("{} {} STL", attachment.id, channel_label),
            format: "stl".to_string(),
            path: stl_path.to_string_lossy().to_string(),
            role: "sidecar".to_string(),
        });
        objects.push(ThreeMfObject {
            id: (channel_index + 2) as u32,
            name: format!("{} Overlay", channel_label),
            color_index: channel_index + 1,
            triangles: mesh,
        });
    }

    if objects.len() <= 1 {
        return Ok(Vec::new());
    }

    let package_path = export_dir.join(format!("{}-cmyk.3mf", attachment.id));
    write_3mf_package(&package_path, &objects)?;
    artifacts.insert(
        0,
        ExportArtifact {
            label: format!("{} CMYK 3MF", attachment.id),
            format: "3mf".to_string(),
            path: package_path.to_string_lossy().to_string(),
            role: "primary".to_string(),
        },
    );
    Ok(artifacts)
}

fn ensure_preview_file_size_within_limit(path: &Path) -> AppResult<()> {
    let size_bytes = fs::metadata(path)
        .map_err(|e| {
            AppError::internal(format!(
                "Failed to inspect lithophane preview '{}': {}",
                path.display(),
                e
            ))
        })?
        .len();
    if size_bytes > MAX_SAFE_PREVIEW_STL_BYTES {
        return Err(AppError::validation(format!(
            "Lithophane preview is too large for the viewer ({} MB > {} MB). Reduce image coverage or resolution, or apply the image to a smaller flatter patch.",
            size_bytes / (1024 * 1024),
            MAX_SAFE_PREVIEW_STL_BYTES / (1024 * 1024)
        )));
    }
    Ok(())
}

fn write_safe_preview_mesh(output_stl: &Path, mesh: &Mesh) -> AppResult<()> {
    let file_name = output_stl
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("preview.stl");
    let temp_output = output_stl.with_file_name(format!("{}.tmp", file_name));
    write_mesh(&temp_output, mesh)?;
    let size_check = ensure_preview_file_size_within_limit(&temp_output);
    if size_check.is_err() {
        let _ = fs::remove_file(&temp_output);
        return size_check;
    }
    fs::rename(&temp_output, output_stl).map_err(|e| {
        AppError::internal(format!(
            "Failed to replace lithophane preview '{}': {}",
            output_stl.display(),
            e
        ))
    })
}

pub fn apply_lithophane_attachments(
    input_stl: &Path,
    attachments: &[ResolvedLithophaneAttachment],
    output_stl: &Path,
    export_dir: &Path,
) -> AppResult<Vec<ExportArtifact>> {
    let mut mesh = read_mesh(input_stl)?;
    let global_bounds = Bounds::from_vertices(&mesh.vertices);
    let mut export_artifacts = Vec::new();

    for attachment in attachments {
        if attachment.image_path.trim().is_empty() {
            continue;
        }
        let dynamic_image = image::open(&attachment.image_path).map_err(|e| {
            AppError::internal(format!(
                "Failed to open image for lithophane attachment '{}': {}",
                attachment.id, e
            ))
        })?;
        let image_rgb = dynamic_image.to_rgb8();
        let image_luma = dynamic_image.to_luma8();
        let bounds = attachment
            .target_bounds
            .as_ref()
            .map(Bounds::from)
            .unwrap_or(global_bounds);
        let frame = PatchFrame::from_bounds(bounds, &attachment.placement);
        let projection = resolve_projection(&attachment.placement, frame);
        refine_mesh_for_attachment(
            &mut mesh,
            frame,
            projection,
            image_luma.width(),
            image_luma.height(),
        );
        apply_attachment_to_mesh(&mut mesh, frame, attachment, &image_luma);

        if matches!(attachment.color_mode, LithophaneColorMode::Cmyk)
            && matches!(projection, ProjectionType::Planar)
        {
            fs::create_dir_all(export_dir).map_err(|e| {
                AppError::internal(format!(
                    "Failed to create lithophane export dir '{}': {}",
                    export_dir.display(),
                    e
                ))
            })?;
            let mut attachment_artifacts = generate_cmyk_export_artifacts(
                export_dir,
                output_stl,
                attachment,
                frame,
                projection,
                &image_rgb,
                &image_luma,
            )?;
            export_artifacts.append(&mut attachment_artifacts);
        }
    }

    write_safe_preview_mesh(output_stl, &mesh)?;
    Ok(export_artifacts)
}

pub fn resolve_image_path(
    attachment: &LithophaneAttachment,
    parameters: &crate::models::DesignParams,
) -> Option<String> {
    match &attachment.source {
        LithophaneAttachmentSource::File { image_path } => {
            let trimmed = image_path.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        LithophaneAttachmentSource::Param { image_param } => {
            let value = parameters.get(image_param)?;
            match value {
                crate::models::ParamValue::String(path) if !path.trim().is_empty() => {
                    Some(path.trim().to_string())
                }
                _ => None,
            }
        }
    }
}

pub fn export_dir_for_preview(preview_stl_path: &Path) -> PathBuf {
    let stem = preview_stl_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("preview");
    preview_stl_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("{}-exports", stem))
}

#[cfg(test)]
mod tests {
    use super::{
        bilinear_gray, compute_fit_uv, ensure_preview_file_size_within_limit,
        export_dir_for_preview, sample_target_segments, write_3mf_package, Bounds, PatchFrame,
        ThreeMfObject, Vec3, CYLINDRICAL_SEGMENT_CEILING, MAX_SAFE_PREVIEW_STL_BYTES,
    };
    use crate::contracts::{
        LithophanePlacement, LithophanePlacementMode, LithophaneSide, OverflowMode, ProjectionType,
    };
    use image::{GrayImage, Luma};
    use std::fs;
    use std::io::Read;
    use std::path::Path;
    use zip::ZipArchive;

    fn sample_frame(side: LithophaneSide, projection: ProjectionType) -> PatchFrame {
        PatchFrame::from_bounds(
            Bounds {
                min: Vec3 {
                    x: -20.0,
                    y: -10.0,
                    z: 0.0,
                },
                max: Vec3 {
                    x: 20.0,
                    y: 10.0,
                    z: 2.0,
                },
            },
            &LithophanePlacement {
                mode: LithophanePlacementMode::PartSidePatch,
                side,
                projection,
                width_mm: 24.0,
                height_mm: 12.0,
                offset_x_mm: 0.0,
                offset_y_mm: 0.0,
                rotation_deg: 0.0,
                overflow_mode: OverflowMode::Contain,
                bleed_margin_mm: 0.0,
            },
        )
    }

    #[test]
    fn bilinear_sampling_blends_neighbor_pixels() {
        let image = GrayImage::from_fn(2, 2, |x, y| match (x, y) {
            (0, 0) => Luma([0]),
            (1, 0) => Luma([255]),
            (0, 1) => Luma([255]),
            _ => Luma([0]),
        });
        let sample = bilinear_gray(&image, 0.5, 0.5);
        assert!(
            sample > 0.45 && sample < 0.55,
            "unexpected bilinear sample: {}",
            sample
        );
    }

    #[test]
    fn cmyk_3mf_writer_indexes_shared_vertices_so_slicers_keep_mesh_topology() {
        let root =
            std::env::temp_dir().join(format!("ecky-lithophane-3mf-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let package = root.join("indexed.3mf");

        write_3mf_package(
            &package,
            &[ThreeMfObject {
                id: 1,
                name: "Quad".to_string(),
                color_index: 0,
                triangles: vec![
                    [
                        Vec3 {
                            x: 0.0,
                            y: 0.0,
                            z: 0.0,
                        },
                        Vec3 {
                            x: 10.0,
                            y: 0.0,
                            z: 0.0,
                        },
                        Vec3 {
                            x: 0.0,
                            y: 10.0,
                            z: 0.0,
                        },
                    ],
                    [
                        Vec3 {
                            x: 10.0,
                            y: 0.0,
                            z: 0.0,
                        },
                        Vec3 {
                            x: 10.0,
                            y: 10.0,
                            z: 0.0,
                        },
                        Vec3 {
                            x: 0.0,
                            y: 10.0,
                            z: 0.0,
                        },
                    ],
                ],
            }],
        )
        .unwrap();

        let file = fs::File::open(&package).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        let mut model_xml = String::new();
        archive
            .by_name("3D/3dmodel.model")
            .unwrap()
            .read_to_string(&mut model_xml)
            .unwrap();

        assert_eq!(model_xml.matches("<vertex ").count(), 4);
        assert_eq!(model_xml.matches("<triangle ").count(), 2);
        assert!(model_xml.contains(r#"<triangle v1="0" v2="1" v3="2"/>"#));
        assert!(model_xml.contains(r#"<triangle v1="1" v2="3" v3="2"/>"#));
    }

    #[test]
    fn adaptive_segments_are_no_longer_capped_at_64() {
        let segments = sample_target_segments(
            sample_frame(LithophaneSide::Front, ProjectionType::Planar),
            ProjectionType::Planar,
            1200,
            800,
        );
        assert!(segments > 64.0);
    }

    #[test]
    fn contain_fit_preserves_image_inside_patch_bounds() {
        let frame = sample_frame(LithophaneSide::Front, ProjectionType::Planar);
        assert!(compute_fit_uv(frame, 200, 100, 0.5, 0.5).is_some());
        assert!(compute_fit_uv(frame, 100, 200, 0.05, 0.5).is_none());
    }

    #[test]
    fn cylindrical_segments_are_capped_for_viewer_safety() {
        let frame = PatchFrame::from_bounds(
            Bounds {
                min: Vec3 {
                    x: -120.0,
                    y: -120.0,
                    z: 0.0,
                },
                max: Vec3 {
                    x: 120.0,
                    y: 120.0,
                    z: 200.0,
                },
            },
            &LithophanePlacement {
                mode: LithophanePlacementMode::PartSidePatch,
                side: LithophaneSide::Front,
                projection: ProjectionType::Cylindrical,
                width_mm: 0.0,
                height_mm: 0.0,
                offset_x_mm: 0.0,
                offset_y_mm: 0.0,
                rotation_deg: 0.0,
                overflow_mode: OverflowMode::Contain,
                bleed_margin_mm: 0.0,
            },
        );
        let segments = sample_target_segments(frame, ProjectionType::Cylindrical, 4000, 3000);
        assert!(segments <= CYLINDRICAL_SEGMENT_CEILING);
    }

    #[test]
    fn part_side_patch_transform_supports_all_sides() {
        let sides = [
            LithophaneSide::Front,
            LithophaneSide::Back,
            LithophaneSide::Left,
            LithophaneSide::Right,
            LithophaneSide::Top,
            LithophaneSide::Bottom,
        ];
        for side in sides {
            let frame = sample_frame(side, ProjectionType::Planar);
            let point = frame.point(0.5, 0.5, 0.0);
            match side {
                LithophaneSide::Front => assert!((point.y - 10.0).abs() < 1e-4),
                LithophaneSide::Back => assert!((point.y + 10.0).abs() < 1e-4),
                LithophaneSide::Left => assert!((point.x + 20.0).abs() < 1e-4),
                LithophaneSide::Right => assert!((point.x - 20.0).abs() < 1e-4),
                LithophaneSide::Top => assert!((point.z - 2.0).abs() < 1e-4),
                LithophaneSide::Bottom => assert!(point.z.abs() < 1e-4),
            }
        }
    }

    #[test]
    fn export_dir_uses_local_preview_stem() {
        let path = export_dir_for_preview(Path::new("/tmp/example-preview.stl"));
        assert_eq!(path, Path::new("/tmp/example-preview-exports"));
    }

    #[test]
    fn oversized_preview_file_is_rejected() {
        let root = std::env::temp_dir().join(format!("ecky-litho-size-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let preview = root.join("preview.stl");
        let file = std::fs::File::create(&preview).unwrap();
        file.set_len(MAX_SAFE_PREVIEW_STL_BYTES + 1).unwrap();
        let error = ensure_preview_file_size_within_limit(&preview).unwrap_err();
        assert!(error.to_string().contains("too large for the viewer"));
    }
}
