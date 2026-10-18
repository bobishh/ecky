use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use tauri::{AppHandle, State};

use crate::contracts::{
    AppError, AppResult, ArtifactBundle, BrepHiddenLineProjectionRequest,
    BrepHiddenLineProjectionResponse, DesignOutput, DesignParams, ExportPartInput,
    FreecadLibraryImportRequest, MacroDialect, ModelManifest, ModelSourceKind,
};
use crate::db;
use crate::freecad;
use crate::models::AppState;
use crate::services::session::write_last_snapshot;

fn export_part_label(part: &ExportPartInput) -> String {
    let label = part.label.trim();
    if !label.is_empty() {
        return label.to_string();
    }
    if let Some(object_name) = part.object_name.as_deref() {
        if !object_name.trim().is_empty() {
            return object_name.trim().to_string();
        }
    }
    if let Some(part_id) = part.part_id.as_deref() {
        if !part_id.trim().is_empty() {
            return part_id.trim().to_string();
        }
    }
    "Part".to_string()
}

fn export_object_name(part: &ExportPartInput, index: usize) -> String {
    let label = export_part_label(part);
    if label == "Part" {
        format!("Part {}", index + 1)
    } else {
        label
    }
}

fn sanitize_export_stem(input: &str) -> String {
    let mut sanitized = String::new();
    let mut previous_dash = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            sanitized.push(ch.to_ascii_lowercase());
            previous_dash = false;
        } else if !previous_dash {
            sanitized.push('-');
            previous_dash = true;
        }
    }
    sanitized.trim_matches('-').to_string()
}

fn export_entry_name(index: usize, part: &ExportPartInput) -> String {
    let stem = sanitize_export_stem(&export_part_label(part));
    let suffix = if stem.is_empty() {
        "part"
    } else {
        stem.as_str()
    };
    format!("{:02}-{}.stl", index + 1, suffix)
}

fn export_body_entry_name(index: usize, part: &ExportPartInput, body_index: usize) -> String {
    let stem = sanitize_export_stem(&export_part_label(part));
    let suffix = if stem.is_empty() {
        "part"
    } else {
        stem.as_str()
    };
    format!("{:02}-{}-body-{:02}.stl", index + 1, suffix, body_index + 1)
}

fn normalize_display_color(color: Option<&str>) -> String {
    let Some(raw) = color.map(str::trim).filter(|value| !value.is_empty()) else {
        return "#D8D8D8FF".to_string();
    };
    let digits = raw.strip_prefix('#').unwrap_or(raw);
    match digits.len() {
        6 if digits.chars().all(|ch| ch.is_ascii_hexdigit()) => {
            format!("#{}FF", digits.to_ascii_uppercase())
        }
        8 if digits.chars().all(|ch| ch.is_ascii_hexdigit()) => {
            format!("#{}", digits.to_ascii_uppercase())
        }
        _ => "#D8D8D8FF".to_string(),
    }
}

fn ensure_exportable_parts(parts: &[ExportPartInput]) -> AppResult<()> {
    if parts.len() < 2 {
        return Err(AppError::validation(
            "Multipart export requires at least two parts.",
        ));
    }

    for part in parts {
        let path = part.path.trim();
        if path.is_empty() {
            return Err(AppError::validation(format!(
                "Multipart export part '{}' is missing a source path.",
                export_part_label(part)
            )));
        }
        let metadata = fs::metadata(path).map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                AppError::not_found(format!(
                    "Export part '{}' was not found at '{}'.",
                    export_part_label(part),
                    path
                ))
            } else {
                AppError::persistence(format!(
                    "Failed to inspect export part '{}' at '{}': {}",
                    export_part_label(part),
                    path,
                    err
                ))
            }
        })?;
        if !metadata.is_file() {
            return Err(AppError::validation(format!(
                "Export part '{}' at '{}' is not a file.",
                export_part_label(part),
                path
            )));
        }
    }
    Ok(())
}

fn ensure_target_parent_dir(target_path: &Path) -> AppResult<()> {
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            AppError::persistence(format!(
                "Failed to prepare export directory '{}': {}",
                parent.display(),
                err
            ))
        })?;
    }
    Ok(())
}

pub(crate) fn export_multipart_stl_zip_impl(
    parts: &[ExportPartInput],
    target_path: &str,
    _model_name: String,
) -> AppResult<()> {
    ensure_exportable_parts(parts)?;
    if let Some(bundle) = try_mesh_native_bundle_from_adjacent_sidecars(parts)? {
        return export_mesh_native_bundle_as_stl_zip_impl(&bundle, target_path);
    }
    let target = Path::new(target_path);
    ensure_target_parent_dir(target)?;

    let file = File::create(target).map_err(|err| {
        AppError::persistence(format!(
            "Failed to create multipart STL archive '{}': {}",
            target.display(),
            err
        ))
    })?;
    let mut zip = zip::ZipWriter::new(file);
    let options =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let mut output_index = 0usize;
    for part in parts {
        let triangles = read_binary_stl_triangles(Path::new(&part.path))?;
        let triangles = if let Some(frame) = part.placement_frame.as_ref() {
            transform_stl_triangles(triangles, frame)
        } else {
            triangles
        };
        let bodies = split_stl_triangle_components(triangles);
        let split = bodies.len() > 1;

        for (body_index, body_triangles) in bodies.into_iter().enumerate() {
            let entry_name = if split {
                export_body_entry_name(output_index, part, body_index)
            } else {
                export_entry_name(output_index, part)
            };
            output_index += 1;
            zip.start_file(entry_name, options).map_err(|err| {
                AppError::persistence(format!(
                    "Failed to add '{}' to multipart STL archive: {}",
                    export_part_label(part),
                    err
                ))
            })?;
            let body_triangles = if part.placement_frame.is_some() {
                body_triangles
            } else {
                localize_stl_triangles(body_triangles).0
            };
            write_binary_stl_triangles(&mut zip, &body_triangles).map_err(|err| {
                AppError::persistence(format!(
                    "Failed to write '{}' into multipart STL archive: {}",
                    export_part_label(part),
                    err
                ))
            })?;
        }
    }

    zip.finish().map_err(|err| {
        AppError::persistence(format!(
            "Failed to finalize multipart STL archive '{}': {}",
            target.display(),
            err
        ))
    })?;
    Ok(())
}

#[derive(Clone)]
struct MultipartThreeMfObject {
    id: u32,
    name: String,
    color_index: usize,
    transform: Option<String>,
    vertices: Vec<[f32; 3]>,
    triangles: Vec<[usize; 3]>,
}

fn three_mf_vertex_key(vertex: [f32; 3]) -> [i64; 3] {
    vertex.map(|value| (value as f64 * 100_000.0).round() as i64)
}

fn indexed_three_mf_mesh(triangles: Vec<[[f32; 3]; 3]>) -> (Vec<[f32; 3]>, Vec<[usize; 3]>) {
    let mut vertices = Vec::<[f32; 3]>::new();
    let mut index_by_key = std::collections::BTreeMap::<[i64; 3], usize>::new();
    let mut indexed_triangles = Vec::<[usize; 3]>::with_capacity(triangles.len());

    for triangle in triangles {
        let mut indices = [0usize; 3];
        for (slot, vertex) in triangle.into_iter().enumerate() {
            let key = three_mf_vertex_key(vertex);
            let index = if let Some(index) = index_by_key.get(&key).copied() {
                index
            } else {
                let index = vertices.len();
                vertices.push(vertex);
                index_by_key.insert(key, index);
                index
            };
            indices[slot] = index;
        }
        indexed_triangles.push(indices);
    }

    (vertices, indexed_triangles)
}

fn split_stl_triangle_components(triangles: Vec<[[f32; 3]; 3]>) -> Vec<Vec<[[f32; 3]; 3]>> {
    type VertexKey = [i64; 3];
    type EdgeKey = (VertexKey, VertexKey);

    if triangles.is_empty() {
        return vec![triangles];
    }

    let vertex_key =
        |vertex: [f32; 3]| vertex.map(|value| (value as f64 * 10_000.0).round() as i64);
    let edge_key = |left: VertexKey, right: VertexKey| {
        if left <= right {
            (left, right)
        } else {
            (right, left)
        }
    };

    let mut edge_triangles = std::collections::BTreeMap::<EdgeKey, Vec<usize>>::new();
    for (triangle_index, triangle) in triangles.iter().enumerate() {
        let vertices = triangle.map(vertex_key);
        for edge in [
            edge_key(vertices[0], vertices[1]),
            edge_key(vertices[1], vertices[2]),
            edge_key(vertices[2], vertices[0]),
        ] {
            edge_triangles.entry(edge).or_default().push(triangle_index);
        }
    }

    let mut neighbors = vec![Vec::<usize>::new(); triangles.len()];
    for triangle_ids in edge_triangles.values() {
        for &left in triangle_ids {
            for &right in triangle_ids {
                if left != right {
                    neighbors[left].push(right);
                }
            }
        }
    }

    let mut visited = vec![false; triangles.len()];
    let mut components = Vec::new();
    for seed in 0..triangles.len() {
        if visited[seed] {
            continue;
        }
        visited[seed] = true;
        let mut stack = vec![seed];
        let mut triangle_ids = Vec::new();
        while let Some(triangle_index) = stack.pop() {
            triangle_ids.push(triangle_index);
            for &neighbor in &neighbors[triangle_index] {
                if !visited[neighbor] {
                    visited[neighbor] = true;
                    stack.push(neighbor);
                }
            }
        }
        triangle_ids.sort_unstable();
        components.push(
            triangle_ids
                .into_iter()
                .map(|triangle_index| triangles[triangle_index])
                .collect(),
        );
    }
    components
}

fn read_binary_stl_triangles(path: &Path) -> AppResult<Vec<[[f32; 3]; 3]>> {
    let bytes = fs::read(path).map_err(|err| {
        AppError::not_found(format!(
            "Failed to open STL part '{}' for multipart export: {}",
            path.display(),
            err
        ))
    })?;
    if bytes.len() < 84 {
        return Err(AppError::internal(format!(
            "STL part '{}' is too small ({}) to be a valid binary STL while exporting multipart model.",
            path.display(),
            bytes.len()
        )));
    }
    // ASCII STL starts with "solid". Detect it explicitly so callers get a clear
    // error instead of the cryptic read_exact EOF ("failed to fill whole buffer")
    // that results from reading ASCII text as a triangle count.
    let header_is_ascii = bytes.starts_with(b"solid");
    let triangle_count = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
    let expected_binary_len = 84 + triangle_count * 50;
    if header_is_ascii && bytes.len() != expected_binary_len {
        return Err(AppError::internal(format!(
            "STL part '{}' appears to be ASCII STL. Multipart export requires binary STL. Re-render the model to regenerate binary part files. (path: {})",
            path.file_name().map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string()),
            path.display()
        )));
    }
    if bytes.len() != expected_binary_len {
        return Err(AppError::internal(format!(
            "STL part '{}' is malformed: header declares {} triangles (expects {} bytes) but the file is {} bytes while exporting multipart model.",
            path.display(),
            triangle_count,
            expected_binary_len,
            bytes.len()
        )));
    }
    let mut triangles = Vec::with_capacity(triangle_count);
    let mut off = 84usize;
    for _ in 0..triangle_count {
        off += 12; // normal
        let a = [
            f32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()),
            f32::from_le_bytes(bytes[off + 4..off + 8].try_into().unwrap()),
            f32::from_le_bytes(bytes[off + 8..off + 12].try_into().unwrap()),
        ];
        off += 12;
        let b = [
            f32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()),
            f32::from_le_bytes(bytes[off + 4..off + 8].try_into().unwrap()),
            f32::from_le_bytes(bytes[off + 8..off + 12].try_into().unwrap()),
        ];
        off += 12;
        let c = [
            f32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()),
            f32::from_le_bytes(bytes[off + 4..off + 8].try_into().unwrap()),
            f32::from_le_bytes(bytes[off + 8..off + 12].try_into().unwrap()),
        ];
        off += 12 + 2; // last vertex + attribute
        triangles.push([a, b, c]);
    }
    Ok(triangles)
}

fn transform_stl_triangles(
    triangles: Vec<[[f32; 3]; 3]>,
    frame: &crate::contracts::PortFrame,
) -> Vec<[[f32; 3]; 3]> {
    triangles
        .into_iter()
        .map(|triangle| triangle.map(|vertex| transform_stl_vertex(vertex, frame)))
        .collect()
}

fn localize_stl_triangles(mut triangles: Vec<[[f32; 3]; 3]>) -> (Vec<[[f32; 3]; 3]>, [f32; 3]) {
    let min = triangles.iter().flat_map(|triangle| triangle.iter()).fold(
        [f32::INFINITY; 3],
        |mut acc, vertex| {
            for axis in 0..3 {
                acc[axis] = acc[axis].min(vertex[axis]);
            }
            acc
        },
    );

    if min.iter().any(|value| !value.is_finite()) {
        return (triangles, [0.0, 0.0, 0.0]);
    }

    for triangle in &mut triangles {
        for vertex in triangle {
            for axis in 0..3 {
                vertex[axis] -= min[axis];
            }
        }
    }

    (triangles, min)
}

fn transform_stl_vertex(vertex: [f32; 3], frame: &crate::contracts::PortFrame) -> [f32; 3] {
    [
        (frame.origin[0]
            + frame.x_axis[0] * vertex[0] as f64
            + frame.y_axis[0] * vertex[1] as f64
            + frame.z_axis[0] * vertex[2] as f64) as f32,
        (frame.origin[1]
            + frame.x_axis[1] * vertex[0] as f64
            + frame.y_axis[1] * vertex[1] as f64
            + frame.z_axis[1] * vertex[2] as f64) as f32,
        (frame.origin[2]
            + frame.x_axis[2] * vertex[0] as f64
            + frame.y_axis[2] * vertex[1] as f64
            + frame.z_axis[2] * vertex[2] as f64) as f32,
    ]
}

fn triangle_normal(triangle: &[[f32; 3]; 3]) -> [f32; 3] {
    let ab = [
        triangle[1][0] - triangle[0][0],
        triangle[1][1] - triangle[0][1],
        triangle[1][2] - triangle[0][2],
    ];
    let ac = [
        triangle[2][0] - triangle[0][0],
        triangle[2][1] - triangle[0][1],
        triangle[2][2] - triangle[0][2],
    ];
    let cross = [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ];
    let length = (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
    if length <= f32::EPSILON {
        [0.0, 0.0, 0.0]
    } else {
        [cross[0] / length, cross[1] / length, cross[2] / length]
    }
}

fn write_binary_stl_triangles<W: Write>(
    writer: &mut W,
    triangles: &[[[f32; 3]; 3]],
) -> AppResult<()> {
    let mut header = [0u8; 80];
    let label = b"Ecky multipart STL export";
    header[..label.len()].copy_from_slice(label);
    writer.write_all(&header).map_err(|err| {
        AppError::persistence(format!(
            "Failed to write STL header during multipart export: {}",
            err
        ))
    })?;
    writer
        .write_all(&(triangles.len() as u32).to_le_bytes())
        .map_err(|err| {
            AppError::persistence(format!(
                "Failed to write STL triangle count during multipart export: {}",
                err
            ))
        })?;
    for triangle in triangles {
        let normal = triangle_normal(triangle);
        for scalar in normal {
            writer.write_all(&scalar.to_le_bytes()).map_err(|err| {
                AppError::persistence(format!(
                    "Failed to write STL normal during multipart export: {}",
                    err
                ))
            })?;
        }
        for vertex in triangle {
            for scalar in vertex {
                writer.write_all(&scalar.to_le_bytes()).map_err(|err| {
                    AppError::persistence(format!(
                        "Failed to write STL vertex during multipart export: {}",
                        err
                    ))
                })?;
            }
        }
        writer.write_all(&0u16.to_le_bytes()).map_err(|err| {
            AppError::persistence(format!(
                "Failed to write STL triangle attribute during multipart export: {}",
                err
            ))
        })?;
    }
    Ok(())
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn format_3mf_transform_scalar(value: f64) -> String {
    if value.abs() <= 1.0e-9 {
        return "0".to_string();
    }
    let rounded = value.round();
    if (value - rounded).abs() <= 1.0e-9 {
        return format!("{}", rounded as i64);
    }
    let mut text = format!("{value:.6}");
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

fn export_part_transform_attr(part: &ExportPartInput) -> Option<String> {
    let frame = part.placement_frame.as_ref()?;
    Some(
        [
            frame.x_axis[0],
            frame.x_axis[1],
            frame.x_axis[2],
            frame.y_axis[0],
            frame.y_axis[1],
            frame.y_axis[2],
            frame.z_axis[0],
            frame.z_axis[1],
            frame.z_axis[2],
            frame.origin[0],
            frame.origin[1],
            frame.origin[2],
        ]
        .into_iter()
        .map(format_3mf_transform_scalar)
        .collect::<Vec<_>>()
        .join(" "),
    )
}

fn export_part_translation_transform_attr(offset: [f32; 3]) -> Option<String> {
    if offset.iter().all(|value| value.abs() <= f32::EPSILON) {
        return None;
    }

    Some(
        [
            1.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
            1.0,
            offset[0] as f64,
            offset[1] as f64,
            offset[2] as f64,
        ]
        .into_iter()
        .map(format_3mf_transform_scalar)
        .collect::<Vec<_>>()
        .join(" "),
    )
}

fn write_multipart_3mf_package(
    path: &Path,
    objects: &[MultipartThreeMfObject],
    colors: &[String],
) -> AppResult<()> {
    let file = File::create(path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to create 3MF export '{}': {}",
            path.display(),
            err
        ))
    })?;
    let mut zip = zip::ZipWriter::new(file);
    let options =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);

    zip.start_file("[Content_Types].xml", options)
        .map_err(|err| {
            AppError::persistence(format!("Failed to write 3MF content types: {}", err))
        })?;
    zip.write_all(br#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/></Types>"#)
        .map_err(|err| AppError::persistence(format!("Failed to write 3MF content types: {}", err)))?;

    zip.add_directory("_rels/", options)
        .map_err(|err| AppError::persistence(format!("Failed to add 3MF rels dir: {}", err)))?;
    zip.start_file("_rels/.rels", options)
        .map_err(|err| AppError::persistence(format!("Failed to write 3MF rels: {}", err)))?;
    zip.write_all(br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Target="/3D/3dmodel.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/></Relationships>"#)
        .map_err(|err| AppError::persistence(format!("Failed to write 3MF rels: {}", err)))?;

    zip.add_directory("3D/", options)
        .map_err(|err| AppError::persistence(format!("Failed to add 3MF 3D dir: {}", err)))?;
    zip.start_file("3D/3dmodel.model", options)
        .map_err(|err| AppError::persistence(format!("Failed to write 3MF model: {}", err)))?;

    let mut xml = String::new();
    xml.push_str(
        r#"<?xml version="1.0" encoding="UTF-8"?><model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02"><resources><basematerials id="1">"#,
    );
    for (index, color) in colors.iter().enumerate() {
        let _ = write!(
            xml,
            r#"<base name="Material {}" displaycolor="{}"/>"#,
            index + 1,
            color
        );
    }
    xml.push_str("</basematerials>");

    for object in objects {
        let _ = write!(
            xml,
            r#"<object id="{}" type="model" pid="1" pindex="{}" name="{}"><mesh><vertices>"#,
            object.id,
            object.color_index,
            xml_escape(&object.name)
        );
        for vertex in &object.vertices {
            let _ = write!(
                xml,
                r#"<vertex x="{:.5}" y="{:.5}" z="{:.5}"/>"#,
                vertex[0], vertex[1], vertex[2]
            );
        }
        xml.push_str("</vertices><triangles>");
        for triangle in &object.triangles {
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
        if let Some(transform) = object.transform.as_ref() {
            let _ = write!(
                xml,
                r#"<item objectid="{}" transform="{}"/>"#,
                object.id, transform
            );
        } else {
            let _ = write!(xml, r#"<item objectid="{}"/>"#, object.id);
        }
    }
    xml.push_str("</build></model>");
    zip.write_all(xml.as_bytes())
        .map_err(|err| AppError::persistence(format!("Failed to write 3MF model XML: {}", err)))?;
    zip.finish()
        .map_err(|err| AppError::persistence(format!("Failed to finalize 3MF export: {}", err)))?;
    Ok(())
}

pub(crate) fn export_multipart_3mf_impl(
    parts: &[ExportPartInput],
    target_path: &str,
    _model_name: String,
) -> AppResult<()> {
    ensure_exportable_parts(parts)?;
    if let Some(bundle) = try_mesh_native_bundle_from_adjacent_sidecars(parts)? {
        return export_mesh_native_bundle_as_3mf_impl(&bundle, target_path);
    }
    let target = Path::new(target_path);
    ensure_target_parent_dir(target)?;

    let mut colors = Vec::<String>::new();
    let mut objects = Vec::<MultipartThreeMfObject>::new();

    for (index, part) in parts.iter().enumerate() {
        let color = normalize_display_color(part.display_color.as_deref());
        let color_index =
            if let Some(existing_index) = colors.iter().position(|candidate| candidate == &color) {
                existing_index
            } else {
                colors.push(color.clone());
                colors.len() - 1
            };
        let (transform, triangles) = if part.placement_frame.is_some() {
            (
                export_part_transform_attr(part),
                read_binary_stl_triangles(Path::new(&part.path))?,
            )
        } else {
            let (triangles, offset) =
                localize_stl_triangles(read_binary_stl_triangles(Path::new(&part.path))?);
            (export_part_translation_transform_attr(offset), triangles)
        };
        let (vertices, triangles) = indexed_three_mf_mesh(triangles);
        objects.push(MultipartThreeMfObject {
            id: (index + 1) as u32,
            name: export_object_name(part, index),
            color_index,
            transform,
            vertices,
            triangles,
        });
    }

    write_multipart_3mf_package(target, &objects, &colors)
}

/// Build a 3MF object directly from a canonical indexed mesh component. Unlike
/// [`export_multipart_3mf_impl`], which re-indexes binary STL soup through a
/// lossy quantizer, this preserves the authored indexed-mesh topology and ties
/// the exported object to the deterministic component identity/provenance that
/// the [`MultipartMeshNativeBundle`] already owns. No STEP is ever produced.
pub(crate) fn export_mesh_native_bundle_as_3mf_impl(
    bundle: &crate::ecky_ir::mesh_asset::MultipartMeshNativeBundle,
    target_path: &str,
) -> AppResult<()> {
    let target = Path::new(target_path);
    ensure_target_parent_dir(target)?;

    let mut colors = Vec::<String>::new();
    let mut objects = Vec::<MultipartThreeMfObject>::new();
    for (index, component) in bundle.components().iter().enumerate() {
        // Components are mesh-native and unplaced at this boundary; the canonical
        // indexed mesh already encodes authored coordinates, so no STL
        // localization or frame transform is applied here.
        let color_index = push_unique_color(&mut colors, None);
        objects.push(mesh_native_component_to_three_mf_object(
            component,
            index,
            color_index,
        ));
    }

    write_multipart_3mf_package(target, &objects, &colors)
}

/// Mesh-native multipart STL zip. Each authored component becomes one binary
/// STL entry driven by its canonical indexed mesh, reusing the existing STL
/// triangle writer. No STEP is ever produced. See
/// [`export_mesh_native_bundle_as_3mf_impl`] for the routing caveat.
pub(crate) fn export_mesh_native_bundle_as_stl_zip_impl(
    bundle: &crate::ecky_ir::mesh_asset::MultipartMeshNativeBundle,
    target_path: &str,
) -> AppResult<()> {
    let target = Path::new(target_path);
    ensure_target_parent_dir(target)?;

    let file = File::create(target).map_err(|err| {
        AppError::persistence(format!(
            "Failed to create mesh-native STL archive '{}': {}",
            target.display(),
            err
        ))
    })?;
    let mut zip = zip::ZipWriter::new(file);
    let options =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    for (index, component) in bundle.components().iter().enumerate() {
        let stem = sanitize_export_stem(component.label());
        let suffix = if stem.is_empty() {
            "part"
        } else {
            stem.as_str()
        };
        let entry = format!("{:02}-{}.stl", index + 1, suffix);
        zip.start_file(&entry, options).map_err(|err| {
            AppError::persistence(format!(
                "Failed to add mesh-native component '{}' to STL archive: {}",
                component.label(),
                err
            ))
        })?;
        let triangles = indexed_mesh_to_stl_triangles(component.asset());
        write_binary_stl_triangles(&mut zip, &triangles).map_err(|err| {
            AppError::persistence(format!(
                "Failed to write mesh-native component '{}' into STL archive: {}",
                component.label(),
                err
            ))
        })?;
    }

    zip.finish().map_err(|err| {
        AppError::persistence(format!(
            "Failed to finalize mesh-native STL archive '{}': {}",
            target.display(),
            err
        ))
    })?;
    Ok(())
}

fn push_unique_color(colors: &mut Vec<String>, raw: Option<&str>) -> usize {
    let color = normalize_display_color(raw);
    if let Some(existing) = colors.iter().position(|candidate| candidate == &color) {
        existing
    } else {
        colors.push(color);
        colors.len() - 1
    }
}

fn mesh_native_component_to_three_mf_object(
    component: &crate::ecky_ir::mesh_asset::MultipartMeshComponent,
    index: usize,
    color_index: usize,
) -> MultipartThreeMfObject {
    let asset = component.asset();
    // Preserve the canonical authored indexing verbatim (u32 -> usize); the
    // only cast is the inherent f64 -> f32 of the 3MF storage format.
    let vertices = asset
        .vertices()
        .iter()
        .map(|vertex| [vertex[0] as f32, vertex[1] as f32, vertex[2] as f32])
        .collect();
    let triangles = asset
        .triangles()
        .iter()
        .map(|triangle| {
            [
                triangle[0] as usize,
                triangle[1] as usize,
                triangle[2] as usize,
            ]
        })
        .collect();
    MultipartThreeMfObject {
        id: (index + 1) as u32,
        name: component.label().to_string(),
        color_index,
        transform: None,
        vertices,
        triangles,
    }
}

fn indexed_mesh_to_stl_triangles(
    asset: &crate::ecky_ir::mesh_asset::IndexedMeshAsset,
) -> Vec<[[f32; 3]; 3]> {
    asset
        .triangles()
        .iter()
        .map(|triangle| {
            let a = asset.vertices()[triangle[0] as usize];
            let b = asset.vertices()[triangle[1] as usize];
            let c = asset.vertices()[triangle[2] as usize];
            [
                [a[0] as f32, a[1] as f32, a[2] as f32],
                [b[0] as f32, b[1] as f32, b[2] as f32],
                [c[0] as f32, c[1] as f32, c[2] as f32],
            ]
        })
        .collect()
}

/// Auto-discover adjacent canonical indexed-mesh sidecars for every export
/// part. A part at `parts/{key}.stl` is paired with `parts/{key}.indexed-mesh.json`,
/// reusing the existing sidecar schema and content-digest validation
/// ([`IndexedMeshAsset::read_cache`]).
///
/// Routing contract — never silently downgrade:
/// - Every part has a valid sidecar → `Ok(Some(bundle))`: route through the
///   mesh-native bundle helpers, preserving canonical topology and
///   deterministic component identity.
/// - Any part has no sidecar → `Ok(None)`: keep the current STL-only path for
///   the whole export (no per-part representation mixing, legacy behavior
///   preserved exactly).
/// - Any part has a present-but-invalid sidecar → `Err`: fail raw and
///   actionable. A broken sidecar signals mesh-native intent and is never
///   silently ignored, even when other parts lack a sidecar.
fn try_mesh_native_bundle_from_adjacent_sidecars(
    parts: &[ExportPartInput],
) -> AppResult<Option<crate::ecky_ir::mesh_asset::MultipartMeshNativeBundle>> {
    use crate::ecky_ir::mesh_asset::{
        IndexedMeshAsset, MultipartMeshComponent, MultipartMeshNativeBundle,
    };

    let mut components = Vec::with_capacity(parts.len());
    let mut any_absent = false;
    for (index, part) in parts.iter().enumerate() {
        let sidecar_path = Path::new(&part.path).with_extension("indexed-mesh.json");
        if !sidecar_path.is_file() {
            any_absent = true;
            continue;
        }
        // The original `MeshAssetSource` provenance is stored in the sidecar
        // schema and read back from it; the caller no longer supplies a source.
        // Deterministic component identity comes from the validated content
        // digest + authored index.
        let asset = IndexedMeshAsset::read_cache(&sidecar_path).map_err(|err| {
                AppError::validation(format!(
                    "Indexed-mesh sidecar '{}' for export part '{}' is malformed and cannot be used: {}. Remove the sidecar to fall back to STL export, or re-render to regenerate it.",
                    sidecar_path.display(),
                    export_part_label(part),
                    err
                ))
            })?;
        components.push(MultipartMeshComponent::new(
            index,
            export_part_label(part),
            asset,
        ));
    }

    if any_absent {
        return Ok(None);
    }
    Ok(Some(MultipartMeshNativeBundle::new(components)?))
}

use crate::services::render::{self as render_service, configured_freecad_cmd};

#[tauri::command]
#[specta::specta]
pub async fn check_freecad(state: State<'_, AppState>, app: AppHandle) -> AppResult<bool> {
    Ok(crate::runtime_capabilities::collect_runtime_capabilities(
        configured_freecad_cmd(&state).as_deref(),
        &app,
    )
    .freecad
    .available)
}

#[tauri::command]
#[specta::specta]
pub async fn get_runtime_capabilities(
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<crate::contracts::RuntimeCapabilities> {
    Ok(crate::runtime_capabilities::collect_runtime_capabilities(
        configured_freecad_cmd(&state).as_deref(),
        &app,
    ))
}

#[tauri::command]
#[specta::specta]
pub async fn render_stl(
    macro_code: String,
    parameters: DesignParams,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<String> {
    render_service::render_stl(&macro_code, &parameters, &state, &app).await
}

#[tauri::command]
#[specta::specta]
pub async fn render_model(
    macro_code: String,
    parameters: DesignParams,
    macro_dialect: Option<MacroDialect>,
    geometry_backend: Option<crate::contracts::GeometryBackend>,
    post_processing: Option<crate::contracts::PostProcessingSpec>,
    previous_manifest: Option<ModelManifest>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<ArtifactBundle> {
    eprintln!(
        "[CAD_FLOW][backend.render_model.start] macro_len={} params={} dialect={:?} backend={:?} previous_model={:?}",
        macro_code.len(),
        parameters.len(),
        macro_dialect,
        geometry_backend,
        previous_manifest.as_ref().map(|manifest| manifest.model_id.as_str()),
    );
    let result = render_service::render_model_with_previous_manifest(
        &macro_code,
        &parameters,
        macro_dialect,
        geometry_backend,
        post_processing.as_ref(),
        previous_manifest.as_ref(),
        &state,
        &app,
    )
    .await;
    match &result {
        Ok(bundle) => eprintln!(
            "[CAD_FLOW][backend.render_model.ok] model_id={} preview={} hash={}",
            bundle.model_id, bundle.model_stl_path, bundle.content_hash
        ),
        Err(error) => eprintln!("[CAD_FLOW][backend.render_model.err] {error}"),
    }
    result
}

#[tauri::command]
#[specta::specta]
pub async fn import_fcstd(
    source_path: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<ArtifactBundle> {
    let _guard = state.acquire_geometry_render().await;
    let result = freecad::import_fcstd(
        &source_path,
        configured_freecad_cmd(&state).as_deref(),
        &app,
    );
    if result.is_ok() {
        let runtime_cache_dir = freecad::runtime_cache_dir(&app)?;
        freecad::evict_cache_if_needed(&runtime_cache_dir);
    }
    result
}

#[tauri::command]
#[specta::specta]
pub async fn import_freecad_library_part(
    request: FreecadLibraryImportRequest,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<ArtifactBundle> {
    let import_path = crate::freecad_library::import_path_from_request(&request)?;
    let source_path = import_path
        .to_str()
        .ok_or_else(|| AppError::internal("Invalid FreeCAD library import path."))?;
    let extension = import_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .unwrap_or_default();

    if !matches!(
        extension.as_str(),
        "fcstd" | "step" | "stp" | "stl" | "obj" | "3mf"
    ) {
        return Err(AppError::validation(format!(
            "FreeCAD library format '{}' is not importable yet.",
            extension
        )));
    }

    let _guard = state.acquire_geometry_render().await;

    if matches!(extension.as_str(), "stl" | "obj" | "3mf") {
        return crate::freecad_library::import_mesh_from_request(&request, &app);
    }

    let result = match extension.as_str() {
        "fcstd" => {
            freecad::import_fcstd(source_path, configured_freecad_cmd(&state).as_deref(), &app)
        }
        "step" | "stp" => {
            freecad::import_step(source_path, configured_freecad_cmd(&state).as_deref(), &app)
        }
        _ => unreachable!("validated FreeCAD library extension"),
    };
    if result.is_ok() {
        let runtime_cache_dir = freecad::runtime_cache_dir(&app)?;
        freecad::evict_cache_if_needed(&runtime_cache_dir);
    }
    result
}

#[tauri::command]
#[specta::specta]
pub async fn apply_imported_model(
    artifact_bundle: ArtifactBundle,
    manifest: ModelManifest,
    parameters: DesignParams,
    message_id: Option<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<ArtifactBundle> {
    let _guard = state.acquire_geometry_render().await;
    let (next_bundle, next_manifest) = freecad::apply_imported_model(
        &artifact_bundle,
        &manifest,
        &parameters,
        configured_freecad_cmd(&state).as_deref(),
        &app,
    )?;

    let mut persisted_output: Option<DesignOutput> = None;
    if let Some(message_id) = message_id.as_ref() {
        let db = state.db.lock().await;
        db::update_message_model_manifest(&db, message_id, &next_manifest).map_err(
            |err: rusqlite::Error| crate::contracts::AppError::persistence(err.to_string()),
        )?;
        db::update_message_artifact_bundle(&db, message_id, &next_bundle).map_err(
            |err: rusqlite::Error| crate::contracts::AppError::persistence(err.to_string()),
        )?;

        let existing_output = db::get_message_output_and_thread(&db, message_id)
            .map_err(|err: rusqlite::Error| {
                crate::contracts::AppError::persistence(err.to_string())
            })?
            .map(|(output, _)| output);
        let mut imported_output = crate::services::imported_model::build_imported_output(
            &next_manifest,
            existing_output.as_ref(),
        );
        imported_output.initial_params = parameters.clone();
        db::update_message_output(&db, message_id, &imported_output)
            .map_err(|err| crate::contracts::AppError::persistence(err.to_string()))?;
        persisted_output = Some(imported_output);
    }

    let snapshot_to_write = {
        let mut last = state.last_snapshot.lock().unwrap();
        if let Some(snapshot) = last.as_mut() {
            let snapshot_matches_model = snapshot
                .model_manifest
                .as_ref()
                .map(|current| current.model_id.as_str() == next_bundle.model_id.as_str())
                .unwrap_or(false)
                || snapshot
                    .artifact_bundle
                    .as_ref()
                    .map(|bundle| bundle.model_id.as_str() == next_bundle.model_id.as_str())
                    .unwrap_or(false);
            let snapshot_matches_message = message_id
                .as_deref()
                .map(|id| snapshot.message_id.as_deref() == Some(id))
                .unwrap_or(true);

            if snapshot_matches_model && snapshot_matches_message {
                snapshot.artifact_bundle = Some(next_bundle.clone());
                snapshot.model_manifest = Some(next_manifest.clone());
                if let Some(output) = persisted_output.clone() {
                    snapshot.design = Some(output);
                }
                Some(snapshot.clone())
            } else {
                None
            }
        } else {
            None
        }
    };

    if let Some(snapshot) = snapshot_to_write.as_ref() {
        write_last_snapshot(&app, Some(snapshot));
    }

    let runtime_cache_dir = freecad::runtime_cache_dir(&app)?;
    freecad::evict_cache_if_needed(&runtime_cache_dir);
    Ok(next_bundle)
}

#[tauri::command]
#[specta::specta]
pub async fn get_model_manifest(model_id: String, app: AppHandle) -> AppResult<ModelManifest> {
    crate::model_runtime::read_model_manifest(&app, &model_id)
}

#[tauri::command]
#[specta::specta]
pub async fn extract_brep_hidden_line_projections(
    request: BrepHiddenLineProjectionRequest,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<BrepHiddenLineProjectionResponse> {
    freecad::extract_brep_hidden_line_projections(
        &app,
        configured_freecad_cmd(&state).as_deref(),
        request,
    )
}

pub(crate) async fn persist_model_manifest(
    model_id: &str,
    manifest: ModelManifest,
    message_id: Option<String>,
    state: &AppState,
    app: &dyn crate::models::PathResolver,
) -> AppResult<ModelManifest> {
    let manifest = crate::model_runtime::write_model_manifest(app, model_id, &manifest)?;
    let refreshed_bundle = crate::model_runtime::read_artifact_bundle(app, model_id).ok();

    let mut persisted_output: Option<DesignOutput> = None;

    if let Some(message_id) = message_id.as_ref() {
        let db = state.db.lock().await;
        db::update_message_model_manifest(&db, message_id, &manifest).map_err(
            |err: rusqlite::Error| crate::contracts::AppError::persistence(err.to_string()),
        )?;
        if let Some(bundle) = refreshed_bundle.as_ref() {
            db::update_message_artifact_bundle(&db, message_id, bundle).map_err(
                |err: rusqlite::Error| crate::contracts::AppError::persistence(err.to_string()),
            )?;
        }

        if matches!(
            manifest.source_kind,
            ModelSourceKind::ImportedFcstd
                | ModelSourceKind::ImportedStep
                | ModelSourceKind::ImportedMesh
        ) {
            let existing_output = db::get_message_output_and_thread(&db, message_id)
                .map_err(|err: rusqlite::Error| {
                    crate::contracts::AppError::persistence(err.to_string())
                })?
                .map(|(output, _)| output);
            let imported_output = crate::services::imported_model::build_imported_output(
                &manifest,
                existing_output.as_ref(),
            );
            db::update_message_output(&db, message_id, &imported_output)
                .map_err(|err| crate::contracts::AppError::persistence(err.to_string()))?;
            persisted_output = Some(imported_output);
        }
    }

    let snapshot_to_write = {
        let mut last = state.last_snapshot.lock().unwrap();
        let Some(snapshot) = last.as_mut() else {
            return Ok(manifest);
        };

        let snapshot_matches_model = snapshot
            .model_manifest
            .as_ref()
            .map(|current| current.model_id.as_str() == model_id)
            .unwrap_or(false)
            || snapshot
                .artifact_bundle
                .as_ref()
                .map(|bundle| bundle.model_id.as_str() == model_id)
                .unwrap_or(false);
        let snapshot_matches_message = message_id
            .as_deref()
            .map(|id| snapshot.message_id.as_deref() == Some(id))
            .unwrap_or(true);

        if snapshot_matches_model && snapshot_matches_message {
            snapshot.model_manifest = Some(manifest.clone());
            if let Some(bundle) = refreshed_bundle.clone() {
                snapshot.artifact_bundle = Some(bundle);
            }
            if let Some(output) = persisted_output.clone() {
                snapshot.design = Some(output);
            }
            Some(snapshot.clone())
        } else {
            None
        }
    };

    if let Some(snapshot) = snapshot_to_write.as_ref() {
        write_last_snapshot(app, Some(snapshot));
    }

    Ok(manifest)
}

pub(crate) async fn load_control_view_target_manifest(
    model_id: &str,
    message_id: Option<&str>,
    state: &AppState,
    app: &dyn crate::models::PathResolver,
) -> AppResult<ModelManifest> {
    let Some(message_id) = message_id else {
        return crate::model_runtime::read_model_manifest(app, model_id);
    };

    let db = state.db.lock().await;
    let (artifact_bundle, model_manifest, _) = db::get_message_runtime_and_thread(&db, message_id)
        .map_err(|error| AppError::persistence(error.to_string()))?
        .ok_or_else(|| AppError::not_found(format!("Message {} not found.", message_id)))?;
    let manifest = model_manifest.ok_or_else(|| {
        AppError::validation(format!("Message {} has no model manifest.", message_id))
    })?;
    if manifest.model_id != model_id {
        return Err(AppError::validation(format!(
            "Message {} manifest modelId '{}' does not match requested model id '{}'.",
            message_id, manifest.model_id, model_id
        )));
    }
    if let Some(bundle) = artifact_bundle.as_ref() {
        crate::contracts::validate_model_runtime_bundle(&manifest, bundle)?;
    }
    Ok(manifest)
}

#[tauri::command]
#[specta::specta]
pub async fn get_default_macro(app: AppHandle) -> AppResult<String> {
    freecad::get_default_macro(&app)
}

#[tauri::command]
#[specta::specta]
pub async fn get_mess_stl_path(app: AppHandle) -> AppResult<String> {
    let path = freecad::resolve_resource_path(
        &app,
        "templates/mess.stl",
        &["../templates/mess.stl", "templates/mess.stl"],
    )?;

    Ok(path
        .to_str()
        .ok_or_else(|| crate::contracts::AppError::internal("Invalid mess STL path."))?
        .to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn export_file(source_path: String, target_path: String) -> AppResult<()> {
    std::fs::copy(&source_path, &target_path).map_err(|err| {
        crate::contracts::AppError::persistence(format!("Failed to export file: {}", err))
    })?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn export_multipart_stl_zip(
    parts: Vec<ExportPartInput>,
    target_path: String,
    model_name: String,
) -> AppResult<()> {
    export_multipart_stl_zip_impl(&parts, &target_path, model_name)
}

#[tauri::command]
#[specta::specta]
pub async fn export_multipart_3mf(
    parts: Vec<ExportPartInput>,
    target_path: String,
    model_name: String,
) -> AppResult<()> {
    export_multipart_3mf_impl(&parts, &target_path, model_name)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Read;
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::contracts::{
        Advisory, AdvisoryCondition, AdvisorySeverity, ControlPrimitive, ControlPrimitiveKind,
        ControlView, ControlViewScope, ControlViewSection, ControlViewSource, DocumentMetadata,
        EnrichmentStatus, ManifestBounds, ManifestEnrichmentState, ParamValue, ParameterGroup,
        PartBinding, PrimitiveBinding, SelectionTarget, SelectionTargetKind, UiField,
        MODEL_RUNTIME_SCHEMA_VERSION,
    };
    use zip::ZipArchive;

    fn temp_export_dir(test_name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "ecky-export-test-{}-{}",
            test_name,
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn write_binary_stl(path: &Path) {
        write_binary_stl_vertices(
            path,
            [[0.0f32, 0.0, 0.0], [10.0f32, 0.0, 0.0], [0.0f32, 10.0, 0.0]],
        );
    }

    fn write_binary_stl_vertices(path: &Path, vertices: [[f32; 3]; 3]) {
        let mut bytes = vec![0u8; 80];
        bytes.extend_from_slice(&(1u32).to_le_bytes());
        bytes.extend_from_slice(&0.0f32.to_le_bytes());
        bytes.extend_from_slice(&0.0f32.to_le_bytes());
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        for vertex in vertices {
            bytes.extend_from_slice(&vertex[0].to_le_bytes());
            bytes.extend_from_slice(&vertex[1].to_le_bytes());
            bytes.extend_from_slice(&vertex[2].to_le_bytes());
        }
        bytes.extend_from_slice(&0u16.to_le_bytes());
        fs::write(path, bytes).unwrap();
    }

    fn write_binary_stl_triangles_to_path(path: &Path, triangles: &[[[f32; 3]; 3]]) {
        let mut bytes = Vec::new();
        write_binary_stl_triangles(&mut bytes, triangles).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn sample_imported_manifest() -> ModelManifest {
        ModelManifest {
            geometry_provenance: None,
            component_import_origins: Vec::new(),
            component_placement_evidence: Vec::new(),
            schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
            model_id: "imported-fcstd-test".to_string(),
            source_kind: ModelSourceKind::ImportedFcstd,
            source_digest: None,
            core_digest: None,
            ast_schema_version: None,
            engine_kind: crate::contracts::EngineKind::Freecad,
            geometry_backend: crate::contracts::GeometryBackend::Freecad,
            source_language: crate::contracts::SourceLanguage::LegacyPython,
            document: DocumentMetadata {
                document_name: "Imported Shell".to_string(),
                document_label: "Imported Shell".to_string(),
                source_path: Some("/tmp/model.FCStd".to_string()),
                object_count: 1,
                warnings: Vec::new(),
            },
            parts: vec![PartBinding {
                part_id: "part-outer-shell".to_string(),
                freecad_object_name: "OuterShell001".to_string(),
                label: "Outer Shell".to_string(),
                kind: "Part::Feature".to_string(),
                semantic_role: Some("body".to_string()),
                viewer_asset_path: Some("/tmp/outer-shell.stl".to_string()),
                viewer_node_ids: vec!["OuterShell001".to_string()],
                parameter_keys: vec![
                    "outer_shell_width".to_string(),
                    "outer_shell_depth".to_string(),
                    "outer_shell_height".to_string(),
                ],
                editable: true,
                bounds: Some(ManifestBounds {
                    x_min: 0.0,
                    y_min: 0.0,
                    z_min: 0.0,
                    x_max: 34.0,
                    y_max: 30.0,
                    z_max: 22.0,
                }),
                volume: None,
                area: None,
            }],
            parameter_groups: vec![ParameterGroup {
                group_id: "proposal-bind-proposal-outershell".to_string(),
                label: "Expose Outer Shell dimensions".to_string(),
                parameter_keys: vec![
                    "outer_shell_width".to_string(),
                    "outer_shell_depth".to_string(),
                    "outer_shell_height".to_string(),
                ],
                part_ids: vec!["part-outer-shell".to_string()],
                editable: true,
                presentation: Some("primary".to_string()),
                order: Some(0),
            }],
            control_primitives: vec![ControlPrimitive {
                primitive_id: "primitive-outer-shell-size".to_string(),
                label: "Outer Shell Size".to_string(),
                kind: ControlPrimitiveKind::Number,
                source: ControlViewSource::Generated,
                part_ids: vec!["part-outer-shell".to_string()],
                bindings: vec![PrimitiveBinding {
                    parameter_key: "outer_shell_width".to_string(),
                    scale: 1.0,
                    offset: 0.0,
                    min: None,
                    max: None,
                }],
                editable: true,
                order: 0,
            }],
            control_relations: Vec::new(),
            control_views: vec![ControlView {
                view_id: "view-outer-shell".to_string(),
                label: "Outer Shell".to_string(),
                scope: ControlViewScope::Part,
                part_ids: vec!["part-outer-shell".to_string()],
                primitive_ids: vec!["primitive-outer-shell-size".to_string()],
                sections: vec![ControlViewSection {
                    section_id: "section-primary".to_string(),
                    label: "Primary".to_string(),
                    primitive_ids: vec!["primitive-outer-shell-size".to_string()],
                    collapsed: false,
                }],
                is_default: true,
                source: ControlViewSource::Generated,
                status: EnrichmentStatus::Accepted,
                order: 0,
            }],
            preview_views: Vec::new(),
            advisories: vec![Advisory {
                advisory_id: "advisory-outer-shell".to_string(),
                label: "Shell note".to_string(),
                severity: AdvisorySeverity::Info,
                primitive_ids: vec!["primitive-outer-shell-size".to_string()],
                view_ids: vec!["view-outer-shell".to_string()],
                message: "Imported shell dimensions drive preview transforms.".to_string(),
                condition: AdvisoryCondition::Always,
                threshold: None,
            }],
            selection_targets: vec![SelectionTarget {
                target_id: Some("target-outer-shell".to_string()),
                durable_target_id: None,
                canonical_target_id: None,
                alias_ids: Vec::new(),
                part_id: "part-outer-shell".to_string(),
                viewer_node_id: "OuterShell001".to_string(),
                label: "Outer Shell".to_string(),
                kind: SelectionTargetKind::Object,
                editable: true,
                parameter_keys: vec![
                    "outer_shell_width".to_string(),
                    "outer_shell_depth".to_string(),
                    "outer_shell_height".to_string(),
                ],
                primitive_ids: vec!["primitive-outer-shell-size".to_string()],
                view_ids: vec!["view-outer-shell".to_string()],
            }],
            measurement_annotations: Vec::new(),
            tagged_anchors: std::collections::BTreeMap::new(),
            feature_graph: None,
            correspondence_graph: None,
            analysis_declarations: Vec::new(),
            warnings: vec![
                "Imported FCStd bindings were accepted from heuristic proposals.".to_string(),
            ],
            enrichment_state: ManifestEnrichmentState {
                status: EnrichmentStatus::Accepted,
                proposals: Vec::new(),
            },
        }
    }

    #[test]
    fn build_imported_output_synthesizes_numeric_controls_from_manifest() {
        let output = crate::services::imported_model::build_imported_output(
            &sample_imported_manifest(),
            None,
        );

        assert_eq!(output.title, "Imported Shell");
        assert_eq!(output.macro_code, "");
        assert_eq!(output.ui_spec.fields.len(), 3);
        assert!(output
            .ui_spec
            .fields
            .iter()
            .all(|field| matches!(field, UiField::Number { .. })));
        assert_eq!(
            output.initial_params.get("outer_shell_width"),
            Some(&ParamValue::Number(34.0))
        );
        assert_eq!(
            output.initial_params.get("outer_shell_depth"),
            Some(&ParamValue::Number(30.0))
        );
        assert_eq!(
            output.initial_params.get("outer_shell_height"),
            Some(&ParamValue::Number(22.0))
        );
    }

    #[test]
    fn export_multipart_stl_zip_packages_parts_with_stable_sanitized_names() {
        let root = temp_export_dir("multipart-zip");
        let body_path = root.join("body.stl");
        let ring_path = root.join("ring.stl");
        let zip_path = root.join("shade-parts.zip");
        write_binary_stl(&body_path);
        write_binary_stl(&ring_path);

        export_multipart_stl_zip_impl(
            &[
                ExportPartInput {
                    label: "Shade Body".to_string(),
                    path: body_path.to_string_lossy().to_string(),
                    object_name: Some("Body".to_string()),
                    part_id: Some("part-body".to_string()),
                    display_color: None,
                    placement_frame: None,
                },
                ExportPartInput {
                    label: "Trim/Ring".to_string(),
                    path: ring_path.to_string_lossy().to_string(),
                    object_name: Some("Ring".to_string()),
                    part_id: Some("part-ring".to_string()),
                    display_color: None,
                    placement_frame: None,
                },
            ],
            zip_path.to_string_lossy().as_ref(),
            "Bulb Lamp Shade".to_string(),
        )
        .unwrap();

        let file = fs::File::open(&zip_path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        let names = (0..archive.len())
            .map(|index| archive.by_index(index).unwrap().name().to_string())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["01-shade-body.stl", "02-trim-ring.stl"]);
    }

    #[test]
    fn export_multipart_stl_zip_splits_disconnected_print_bodies() {
        let root = temp_export_dir("multipart-zip-split-bodies");
        let body_path = root.join("body.stl");
        let pins_path = root.join("pins.stl");
        let zip_path = root.join("dryer-parts.zip");
        let tetra = |x: f32| {
            let a = [x, 0.0, 0.0];
            let b = [x + 1.0, 0.0, 0.0];
            let c = [x, 1.0, 0.0];
            let d = [x, 0.0, 1.0];
            vec![[a, b, c], [a, d, b], [a, c, d], [b, d, c]]
        };
        write_binary_stl_triangles_to_path(&body_path, &tetra(0.0));
        let mut pins = tetra(0.0);
        pins.extend(tetra(10.0));
        write_binary_stl_triangles_to_path(&pins_path, &pins);

        export_multipart_stl_zip_impl(
            &[
                ExportPartInput {
                    label: "Enclosure".to_string(),
                    path: body_path.to_string_lossy().to_string(),
                    object_name: None,
                    part_id: Some("enclosure".to_string()),
                    display_color: None,
                    placement_frame: None,
                },
                ExportPartInput {
                    label: "Catch Pins".to_string(),
                    path: pins_path.to_string_lossy().to_string(),
                    object_name: None,
                    part_id: Some("catch-pins".to_string()),
                    display_color: None,
                    placement_frame: None,
                },
            ],
            zip_path.to_string_lossy().as_ref(),
            "Filament Dryer".to_string(),
        )
        .unwrap();

        let file = fs::File::open(&zip_path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        let names = (0..archive.len())
            .map(|index| archive.by_index(index).unwrap().name().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec![
                "01-enclosure.stl",
                "02-catch-pins-body-01.stl",
                "03-catch-pins-body-02.stl",
            ]
        );
    }

    #[test]
    fn export_multipart_stl_zip_bakes_placement_frame_into_written_stl() {
        let root = temp_export_dir("multipart-zip-transform");
        let body_path = root.join("body.stl");
        let ring_path = root.join("ring.stl");
        let zip_path = root.join("shade-parts.zip");
        let extracted_path = root.join("trim-ring-exported.stl");
        write_binary_stl(&body_path);
        write_binary_stl(&ring_path);

        export_multipart_stl_zip_impl(
            &[
                ExportPartInput {
                    label: "Shade Body".to_string(),
                    path: body_path.to_string_lossy().to_string(),
                    object_name: Some("Body".to_string()),
                    part_id: Some("part-body".to_string()),
                    display_color: None,
                    placement_frame: None,
                },
                ExportPartInput {
                    label: "Trim Ring".to_string(),
                    path: ring_path.to_string_lossy().to_string(),
                    object_name: Some("Ring".to_string()),
                    part_id: Some("part-ring".to_string()),
                    display_color: None,
                    placement_frame: Some(crate::contracts::PortFrame {
                        origin: [12.0, 34.0, 56.0],
                        x_axis: [0.0, 1.0, 0.0],
                        y_axis: [-1.0, 0.0, 0.0],
                        z_axis: [0.0, 0.0, 1.0],
                    }),
                },
            ],
            zip_path.to_string_lossy().as_ref(),
            "Bulb Lamp Shade".to_string(),
        )
        .unwrap();

        let file = fs::File::open(&zip_path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        let mut entry = archive.by_name("02-trim-ring.stl").unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        fs::write(&extracted_path, bytes).unwrap();

        let triangles = read_binary_stl_triangles(&extracted_path).unwrap();
        assert_eq!(
            triangles,
            vec![[[12.0, 34.0, 56.0], [12.0, 44.0, 56.0], [2.0, 34.0, 56.0]]]
        );
    }

    #[test]
    fn export_multipart_stl_zip_localizes_unplaced_part_meshes() {
        let root = temp_export_dir("multipart-zip-localize");
        let body_path = root.join("body.stl");
        let ring_path = root.join("ring.stl");
        let zip_path = root.join("shade-parts.zip");
        let extracted_path = root.join("body-exported.stl");
        write_binary_stl_vertices(
            &body_path,
            [
                [100.0, 200.0, 42.0],
                [110.0, 200.0, 42.0],
                [100.0, 210.0, 42.0],
            ],
        );
        write_binary_stl(&ring_path);

        export_multipart_stl_zip_impl(
            &[
                ExportPartInput {
                    label: "Shade Body".to_string(),
                    path: body_path.to_string_lossy().to_string(),
                    object_name: Some("Body".to_string()),
                    part_id: Some("part-body".to_string()),
                    display_color: None,
                    placement_frame: None,
                },
                ExportPartInput {
                    label: "Trim Ring".to_string(),
                    path: ring_path.to_string_lossy().to_string(),
                    object_name: Some("Ring".to_string()),
                    part_id: Some("part-ring".to_string()),
                    display_color: None,
                    placement_frame: None,
                },
            ],
            zip_path.to_string_lossy().as_ref(),
            "Bulb Lamp Shade".to_string(),
        )
        .unwrap();

        let file = fs::File::open(&zip_path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        let mut entry = archive.by_name("01-shade-body.stl").unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        fs::write(&extracted_path, bytes).unwrap();

        let triangles = read_binary_stl_triangles(&extracted_path).unwrap();
        assert_eq!(
            triangles,
            vec![[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]]]
        );
    }

    #[test]
    fn export_multipart_3mf_writes_all_parts_as_separate_objects_and_colors() {
        let root = temp_export_dir("multipart-3mf");
        let body_path = root.join("body.stl");
        let ring_path = root.join("ring.stl");
        let output_path = root.join("shade.3mf");
        write_binary_stl(&body_path);
        write_binary_stl(&ring_path);

        export_multipart_3mf_impl(
            &[
                ExportPartInput {
                    label: "Shade Body".to_string(),
                    path: body_path.to_string_lossy().to_string(),
                    object_name: Some("Body".to_string()),
                    part_id: Some("part-body".to_string()),
                    display_color: Some("#D8C49AFF".to_string()),
                    placement_frame: None,
                },
                ExportPartInput {
                    label: "Trim Ring".to_string(),
                    path: ring_path.to_string_lossy().to_string(),
                    object_name: Some("Ring".to_string()),
                    part_id: Some("part-ring".to_string()),
                    display_color: Some("#2F4F6FFF".to_string()),
                    placement_frame: Some(crate::contracts::PortFrame {
                        origin: [12.0, 34.0, 56.0],
                        x_axis: [1.0, 0.0, 0.0],
                        y_axis: [0.0, 1.0, 0.0],
                        z_axis: [0.0, 0.0, 1.0],
                    }),
                },
            ],
            output_path.to_string_lossy().as_ref(),
            "Bulb Lamp Shade".to_string(),
        )
        .unwrap();

        let file = fs::File::open(&output_path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        let mut model_xml = String::new();
        archive
            .by_name("3D/3dmodel.model")
            .unwrap()
            .read_to_string(&mut model_xml)
            .unwrap();

        assert!(model_xml.contains("name=\"Shade Body\""));
        assert!(model_xml.contains("name=\"Trim Ring\""));
        assert!(model_xml.contains("displaycolor=\"#D8C49AFF\""));
        assert!(model_xml.contains("displaycolor=\"#2F4F6FFF\""));
        assert!(model_xml.contains("<item objectid=\"1\"/>"));
        assert!(
            model_xml.contains("<item objectid=\"2\" transform=\"1 0 0 0 1 0 0 0 1 12 34 56\"/>")
        );
    }

    #[test]
    fn export_multipart_3mf_indexes_shared_vertices_so_slicers_keep_mesh_topology() {
        let root = temp_export_dir("multipart-3mf-indexed");
        let body_path = root.join("quad.stl");
        let ring_path = root.join("ring.stl");
        let output_path = root.join("quad.3mf");
        write_binary_stl_triangles_to_path(
            &body_path,
            &[
                [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]],
                [[10.0, 0.0, 0.0], [10.0, 10.0, 0.0], [0.0, 10.0, 0.0]],
            ],
        );
        write_binary_stl(&ring_path);

        export_multipart_3mf_impl(
            &[
                ExportPartInput {
                    label: "Shared Quad".to_string(),
                    path: body_path.to_string_lossy().to_string(),
                    object_name: Some("Quad".to_string()),
                    part_id: Some("part-quad".to_string()),
                    display_color: None,
                    placement_frame: None,
                },
                ExportPartInput {
                    label: "Trim Ring".to_string(),
                    path: ring_path.to_string_lossy().to_string(),
                    object_name: Some("Ring".to_string()),
                    part_id: Some("part-ring".to_string()),
                    display_color: None,
                    placement_frame: None,
                },
            ],
            output_path.to_string_lossy().as_ref(),
            "Shared Quad".to_string(),
        )
        .unwrap();

        let file = fs::File::open(&output_path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        let mut model_xml = String::new();
        archive
            .by_name("3D/3dmodel.model")
            .unwrap()
            .read_to_string(&mut model_xml)
            .unwrap();

        let first_object = model_xml.split("</object>").next().unwrap();
        assert_eq!(first_object.matches("<vertex ").count(), 4);
        assert_eq!(first_object.matches("<triangle ").count(), 2);
        assert!(first_object.contains(r#"<triangle v1="0" v2="1" v3="2"/>"#));
        assert!(first_object.contains(r#"<triangle v1="1" v2="3" v3="2"/>"#));
    }

    #[test]
    fn export_multipart_3mf_localizes_unplaced_part_meshes_and_preserves_height_offset() {
        let root = temp_export_dir("multipart-3mf-localize");
        let body_path = root.join("body.stl");
        let ring_path = root.join("ring.stl");
        let output_path = root.join("shade.3mf");
        write_binary_stl_vertices(
            &body_path,
            [
                [100.0, 200.0, 42.0],
                [110.0, 200.0, 42.0],
                [100.0, 210.0, 42.0],
            ],
        );
        write_binary_stl(&ring_path);

        export_multipart_3mf_impl(
            &[
                ExportPartInput {
                    label: "Shade Body".to_string(),
                    path: body_path.to_string_lossy().to_string(),
                    object_name: Some("Body".to_string()),
                    part_id: Some("part-body".to_string()),
                    display_color: None,
                    placement_frame: None,
                },
                ExportPartInput {
                    label: "Trim Ring".to_string(),
                    path: ring_path.to_string_lossy().to_string(),
                    object_name: Some("Ring".to_string()),
                    part_id: Some("part-ring".to_string()),
                    display_color: None,
                    placement_frame: None,
                },
            ],
            output_path.to_string_lossy().as_ref(),
            "Bulb Lamp Shade".to_string(),
        )
        .unwrap();

        let file = fs::File::open(&output_path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        let mut model_xml = String::new();
        archive
            .by_name("3D/3dmodel.model")
            .unwrap()
            .read_to_string(&mut model_xml)
            .unwrap();

        assert!(model_xml.contains(r#"<vertex x="0.00000" y="0.00000" z="0.00000"/>"#));
        assert!(model_xml.contains(r#"transform="1 0 0 0 1 0 0 0 1 100 200 42""#));
        assert!(!model_xml.contains(r#"x="100.00000""#));
        assert!(!model_xml.contains(r#"y="200.00000""#));
        assert!(!model_xml.contains(r#"z="42.00000""#));
    }

    #[test]
    fn multipart_export_fails_clearly_when_part_file_is_missing() {
        let root = temp_export_dir("multipart-missing");
        let body_path = root.join("body.stl");
        let zip_path = root.join("shade-parts.zip");
        write_binary_stl(&body_path);
        let error = export_multipart_stl_zip_impl(
            &[
                ExportPartInput {
                    label: "Shade Body".to_string(),
                    path: body_path.to_string_lossy().to_string(),
                    object_name: Some("Body".to_string()),
                    part_id: Some("part-body".to_string()),
                    display_color: None,
                    placement_frame: None,
                },
                ExportPartInput {
                    label: "Missing Ring".to_string(),
                    path: root.join("missing.stl").to_string_lossy().to_string(),
                    object_name: None,
                    part_id: Some("part-ring".to_string()),
                    display_color: None,
                    placement_frame: None,
                },
            ],
            zip_path.to_string_lossy().as_ref(),
            "Bulb Lamp Shade".to_string(),
        )
        .unwrap_err();

        assert!(
            error.to_string().contains("Missing Ring"),
            "unexpected error: {}",
            error
        );
    }

    fn helper_write_ascii_stl(path: &Path, triangles: &[[[f32; 3]; 3]]) {
        let mut s = String::from("solid ecky\n");
        for [a, b, c] in triangles {
            s.push_str(&format!(
                "facet normal 0 0 1\n  outer loop\n    vertex {} {} {}\n    vertex {} {} {}\n    vertex {} {} {}\n  endloop\nendfacet\n",
                a[0], a[1], a[2], b[0], b[1], b[2], c[0], c[1], c[2],
            ));
        }
        s.push_str("endsolid ecky\n");
        fs::write(path, s).unwrap();
    }

    // --- Mesh-native bundle export wiring (hybrid task 6) ------------------

    fn closed_tetrahedron_indexed_asset(
        source: crate::ecky_ir::mesh_asset::MeshAssetSource,
        offset: [f64; 3],
    ) -> crate::ecky_ir::mesh_asset::IndexedMeshAsset {
        // f32-exact coordinates so the f64 -> f32 3MF cast round-trips bit-exact
        // and the exported geometry can be compared verbatim against the
        // canonical indexed mesh that owns the deterministic identity.
        let vertices = vec![
            [offset[0], offset[1], offset[2]],
            [offset[0] + 4.0, offset[1], offset[2]],
            [offset[0] + 1.0, offset[1] + 3.0, offset[2]],
            [offset[0] + 1.0, offset[1] + 1.0, offset[2] + 3.0],
        ];
        let triangles = vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];
        crate::ecky_ir::mesh_asset::IndexedMeshAsset::new(source, vertices, triangles)
            .expect("closed tetrahedron asset")
    }

    /// Parse every `<object>` block out of a written 3MF model and return, per
    /// object (in document order), its name and its vertex/triangle lists. Used
    /// to prove the mesh-native bundle drove the export from the canonical
    /// indexed mesh rather than re-indexed STL soup.
    fn read_3mf_objects(path: &Path) -> Vec<(String, Vec<[f64; 3]>, Vec<[usize; 3]>)> {
        let file = fs::File::open(path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        let mut model_xml = String::new();
        archive
            .by_name("3D/3dmodel.model")
            .unwrap()
            .read_to_string(&mut model_xml)
            .unwrap();

        let object_re = regex::Regex::new(r"(?s)<object\b.*?</object>").unwrap();
        let attr_re = regex::Regex::new(r#"(\w+)\s*=\s*["']([^"']*)["']"#).unwrap();
        let vertex_re = regex::Regex::new(r"<vertex\b[^>]*>").unwrap();
        let triangle_re = regex::Regex::new(r"<triangle\b[^>]*>").unwrap();

        object_re
            .find_iter(&model_xml)
            .map(|block| {
                let attrs = attr_re
                    .captures_iter(block.as_str())
                    .filter_map(|c| {
                        Some((
                            c.get(1)?.as_str().to_string(),
                            c.get(2)?.as_str().to_string(),
                        ))
                    })
                    .collect::<std::collections::BTreeMap<_, _>>();
                let name = attrs.get("name").cloned().unwrap_or_default();
                let mut vertices = Vec::new();
                for v in vertex_re.find_iter(block.as_str()) {
                    let va = attr_re
                        .captures_iter(v.as_str())
                        .filter_map(|c| {
                            Some((
                                c.get(1)?.as_str().to_string(),
                                c.get(2)?.as_str().to_string(),
                            ))
                        })
                        .collect::<std::collections::BTreeMap<_, _>>();
                    vertices.push([
                        va.get("x").and_then(|s| s.parse().ok()).unwrap_or(0.0),
                        va.get("y").and_then(|s| s.parse().ok()).unwrap_or(0.0),
                        va.get("z").and_then(|s| s.parse().ok()).unwrap_or(0.0),
                    ]);
                }
                let mut triangles = Vec::new();
                for t in triangle_re.find_iter(block.as_str()) {
                    let ta = attr_re
                        .captures_iter(t.as_str())
                        .filter_map(|c| {
                            Some((
                                c.get(1)?.as_str().to_string(),
                                c.get(2)?.as_str().to_string(),
                            ))
                        })
                        .collect::<std::collections::BTreeMap<_, _>>();
                    triangles.push([
                        ta.get("v1").and_then(|s| s.parse().ok()).unwrap_or(0),
                        ta.get("v2").and_then(|s| s.parse().ok()).unwrap_or(0),
                        ta.get("v3").and_then(|s| s.parse().ok()).unwrap_or(0),
                    ]);
                }
                (name, vertices, triangles)
            })
            .collect()
    }

    #[test]
    fn mesh_native_bundle_export_preserves_component_identity_and_provenance_without_step() {
        use crate::ecky_core_ir::NodeId;
        use crate::ecky_ir::mesh_asset::{
            MeshAssetSource, MultipartMeshComponent, MultipartMeshNativeBundle,
        };

        let component_a = MultipartMeshComponent::new(
            0,
            "body",
            closed_tetrahedron_indexed_asset(
                MeshAssetSource::EckyMeshPhase {
                    part_id: "body".to_string(),
                    node_id: NodeId::new(7),
                },
                [0.0, 0.0, 0.0],
            ),
        );
        let component_b = MultipartMeshComponent::new(
            1,
            "imported-island",
            closed_tetrahedron_indexed_asset(MeshAssetSource::Imported, [10.0, 0.0, 0.0]),
        );
        assert_ne!(component_a.content_digest(), component_b.content_digest());
        let bundle = MultipartMeshNativeBundle::new(vec![component_a.clone(), component_b.clone()])
            .expect("mesh-native bundle");

        let root = temp_export_dir("mesh-native-bundle-3mf");
        let output_path = root.join("assembly.3mf");

        // The real multipart export path, driven by the canonical bundle.
        export_mesh_native_bundle_as_3mf_impl(&bundle, output_path.to_string_lossy().as_ref())
            .expect("mesh-native 3MF export");

        // No fabricated STEP: the mesh-native export writes only the 3MF.
        assert!(output_path.is_file());
        assert!(!root.join("model.step").exists());
        assert!(!output_path.with_extension("step").exists());

        let objects = read_3mf_objects(&output_path);
        assert_eq!(objects.len(), 2, "every authored component is exported");

        // Component identity/order is preserved: objects appear in bundle order
        // and carry the canonical component label.
        assert_eq!(objects[0].0, "body");
        assert_eq!(objects[1].0, "imported-island");

        // Provenance is preserved by construction: each exported object's
        // geometry is byte-exact to the canonical indexed mesh that owns the
        // deterministic digest, proving the export consumed the canonical mesh
        // (not re-indexed STL soup).
        for (component, (name, vertices, triangles)) in
            bundle.components().iter().zip(objects.iter())
        {
            assert_eq!(name, component.label());
            let canonical_vertices = component.asset().vertices().to_vec();
            assert_eq!(
                vertices, &canonical_vertices,
                "object '{}' vertices must match the canonical indexed mesh",
                name
            );
            let canonical_triangles: Vec<[usize; 3]> = component
                .asset()
                .triangles()
                .iter()
                .map(|t| [t[0] as usize, t[1] as usize, t[2] as usize])
                .collect();
            assert_eq!(
                triangles, &canonical_triangles,
                "object '{}' triangle topology must match the canonical indexed mesh",
                name
            );
        }

        // Deterministic identity: re-exporting the same bundle reproduces the
        // exact same artifact bytes.
        let first_bytes = fs::read(&output_path).unwrap();
        let replay_path = root.join("assembly-replay.3mf");
        export_mesh_native_bundle_as_3mf_impl(&bundle, replay_path.to_string_lossy().as_ref())
            .expect("replay export");
        let replay_bytes = fs::read(&replay_path).unwrap();
        assert_eq!(
            first_bytes, replay_bytes,
            "identical bundle must reproduce identical export bytes"
        );
    }

    #[test]
    fn mesh_native_bundle_export_as_stl_zip_preserves_each_component_without_step() {
        use crate::ecky_ir::mesh_asset::{
            MeshAssetSource, MultipartMeshComponent, MultipartMeshNativeBundle,
        };

        let bundle = MultipartMeshNativeBundle::new(vec![
            MultipartMeshComponent::new(
                0,
                "body",
                closed_tetrahedron_indexed_asset(
                    MeshAssetSource::EckyMeshPhase {
                        part_id: "body".to_string(),
                        node_id: crate::ecky_core_ir::NodeId::new(7),
                    },
                    [0.0, 0.0, 0.0],
                ),
            ),
            MultipartMeshComponent::new(
                1,
                "imported-island",
                closed_tetrahedron_indexed_asset(MeshAssetSource::Imported, [10.0, 0.0, 0.0]),
            ),
        ])
        .expect("mesh-native bundle");

        let root = temp_export_dir("mesh-native-bundle-stl-zip");
        let output_path = root.join("assembly.zip");
        export_mesh_native_bundle_as_stl_zip_impl(&bundle, output_path.to_string_lossy().as_ref())
            .expect("mesh-native STL zip export");
        assert!(output_path.is_file());
        assert!(!root.join("model.step").exists());

        // Every component is present as its own STL entry, in authored order.
        let file = fs::File::open(&output_path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        let names = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect::<Vec<_>>();
        assert!(names.iter().any(|n| n.contains("body")));
        assert!(names.iter().any(|n| n.contains("imported-island")));
        assert_eq!(names.iter().filter(|n| n.ends_with(".stl")).count(), 2);
    }

    // --- Adjacent indexed-mesh sidecar routing (hybrid task 6, public path) ---

    fn canonical_tetrahedron() -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
        // Authored indexing chosen so STL first-encounter re-indexing reproduces
        // a DIFFERENT triangle index space than the canonical sidecar:
        // canonical triangles [[0,2,1],...] vs STL-reindexed [[0,1,2],...].
        (
            vec![
                [0.0, 0.0, 0.0],
                [4.0, 0.0, 0.0],
                [1.0, 3.0, 0.0],
                [1.0, 1.0, 3.0],
            ],
            vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
        )
    }

    /// STL-reindexed triangle index space that the current legacy path produces
    /// for [`canonical_tetrahedron`]. Used to prove which path ran.
    fn stl_reindexed_tetrahedron_triangles() -> Vec<[usize; 3]> {
        vec![[0, 1, 2], [0, 2, 3], [0, 3, 1], [2, 1, 3]]
    }

    fn write_part_with_adjacent_sidecar(
        dir: &Path,
        key: &str,
        vertices: &[[f64; 3]],
        triangles: &[[u32; 3]],
    ) -> PathBuf {
        use crate::ecky_ir::mesh_asset::{IndexedMeshAsset, MeshAssetSource};
        let stl_path = dir.join(format!("{key}.stl"));
        let soup: Vec<[[f32; 3]; 3]> = triangles
            .iter()
            .map(|triangle| {
                let a = vertices[triangle[0] as usize];
                let b = vertices[triangle[1] as usize];
                let c = vertices[triangle[2] as usize];
                [
                    [a[0] as f32, a[1] as f32, a[2] as f32],
                    [b[0] as f32, b[1] as f32, b[2] as f32],
                    [c[0] as f32, c[1] as f32, c[2] as f32],
                ]
            })
            .collect();
        write_binary_stl_triangles_to_path(&stl_path, &soup);
        let sidecar_path = stl_path.with_extension("indexed-mesh.json");
        IndexedMeshAsset::new(
            MeshAssetSource::Imported,
            vertices.to_vec(),
            triangles.to_vec(),
        )
        .expect("canonical indexed asset")
        .write_cache(&sidecar_path)
        .expect("write adjacent sidecar");
        stl_path
    }

    fn tamper_sidecar_digest(sidecar_path: &Path) {
        let raw = fs::read_to_string(sidecar_path).expect("read sidecar");
        let tampered = raw.replacen("sha256:", "sha256:tampered-", 1);
        fs::write(sidecar_path, tampered).expect("tamper sidecar");
    }

    fn plain_export_part(label: &str, path: &Path) -> ExportPartInput {
        ExportPartInput {
            label: label.to_string(),
            path: path.to_string_lossy().to_string(),
            object_name: None,
            part_id: None,
            display_color: None,
            placement_frame: None,
        }
    }

    /// Extract one STL part entry from a multipart STL zip and decode its
    /// triangles. Reuses the production binary-STL decoder so the proof stays
    /// faithful to what a slicer would see.
    fn read_stl_triangles_from_zip_entry(
        zip_path: &Path,
        name_contains: &str,
    ) -> Vec<[[f32; 3]; 3]> {
        let file = fs::File::open(zip_path).expect("open zip");
        let mut archive = ZipArchive::new(file).expect("zip archive");
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).expect("zip entry");
            if !entry.name().contains(name_contains) {
                continue;
            }
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).expect("read entry");
            let staging =
                std::env::temp_dir().join(format!("ecky-zip-stl-{}.stl", uuid::Uuid::new_v4()));
            fs::write(&staging, &bytes).expect("stage entry");
            let triangles = read_binary_stl_triangles(&staging).expect("decode entry");
            let _ = fs::remove_file(&staging);
            return triangles;
        }
        panic!("zip entry containing '{name_contains}' not found");
    }

    #[test]
    fn export_multipart_3mf_uses_adjacent_indexed_mesh_sidecar_preserving_canonical_topology() {
        let root = temp_export_dir("multipart-sidecar-3mf");
        let (body_vertices, body_triangles) = canonical_tetrahedron();
        let body_stl =
            write_part_with_adjacent_sidecar(&root, "body", &body_vertices, &body_triangles);
        let island_vertices = body_vertices
            .iter()
            .map(|vertex| [vertex[0] + 10.0, vertex[1], vertex[2]])
            .collect::<Vec<_>>();
        let island_stl =
            write_part_with_adjacent_sidecar(&root, "island", &island_vertices, &body_triangles);

        let target = root.join("assembly.3mf");
        export_multipart_3mf_impl(
            &[
                plain_export_part("body", &body_stl),
                plain_export_part("island", &island_stl),
            ],
            target.to_string_lossy().as_ref(),
            "Assembly".to_string(),
        )
        .expect("mesh-native sidecar 3MF export");

        // No fabricated STEP: only the 3MF was written.
        assert!(target.is_file());
        assert!(!root.join("model.step").exists());
        assert!(!target.with_extension("step").exists());

        let objects = read_3mf_objects(&target);
        assert_eq!(objects.len(), 2, "every authored component is exported");
        assert_eq!(objects[0].0, "body");
        assert_eq!(objects[1].0, "island");

        // Canonical topology preserved byte-exact. The STL legacy path would
        // re-index into `stl_reindexed_tetrahedron_triangles()`; the sidecar
        // path preserves the authored index space.
        let canonical: Vec<[usize; 3]> = body_triangles
            .iter()
            .map(|triangle| {
                [
                    triangle[0] as usize,
                    triangle[1] as usize,
                    triangle[2] as usize,
                ]
            })
            .collect();
        assert_eq!(
            objects[0].2, canonical,
            "body triangles must match the canonical sidecar, not STL re-indexing"
        );
        assert_eq!(
            objects[1].2, canonical,
            "island triangles must match the canonical sidecar, not STL re-indexing"
        );
        assert_ne!(objects[0].2, stl_reindexed_tetrahedron_triangles());
        assert_eq!(objects[0].1, body_vertices);
        assert_eq!(objects[1].1, island_vertices);

        // Deterministic identity: identical inputs reproduce identical bytes.
        let first = fs::read(&target).expect("first bytes");
        let replay = root.join("assembly-replay.3mf");
        export_multipart_3mf_impl(
            &[
                plain_export_part("body", &body_stl),
                plain_export_part("island", &island_stl),
            ],
            replay.to_string_lossy().as_ref(),
            "Assembly".to_string(),
        )
        .expect("replay export");
        assert_eq!(first, fs::read(&replay).expect("replay bytes"));
    }

    #[test]
    fn export_multipart_stl_zip_uses_adjacent_indexed_mesh_sidecar_per_component() {
        let root = temp_export_dir("multipart-sidecar-stl-zip");
        let (body_vertices, body_triangles) = canonical_tetrahedron();
        let body_stl =
            write_part_with_adjacent_sidecar(&root, "body", &body_vertices, &body_triangles);
        let island_vertices = body_vertices
            .iter()
            .map(|vertex| [vertex[0] + 10.0, vertex[1], vertex[2]])
            .collect::<Vec<_>>();
        let island_stl =
            write_part_with_adjacent_sidecar(&root, "island", &island_vertices, &body_triangles);

        let target = root.join("assembly.zip");
        export_multipart_stl_zip_impl(
            &[
                plain_export_part("body", &body_stl),
                plain_export_part("island", &island_stl),
            ],
            target.to_string_lossy().as_ref(),
            "Assembly".to_string(),
        )
        .expect("mesh-native sidecar STL zip export");

        assert!(target.is_file());
        assert!(!root.join("model.step").exists());

        let file = fs::File::open(&target).expect("open zip");
        let mut archive = ZipArchive::new(file).expect("zip");
        let names = (0..archive.len())
            .map(|index| archive.by_index(index).unwrap().name().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            names.iter().filter(|name| name.ends_with(".stl")).count(),
            2
        );
        assert!(names.iter().any(|name| name.contains("body")));
        assert!(names.iter().any(|name| name.contains("island")));

        // Routing discriminator: the mesh-native bundle path writes each
        // component's canonical indexed mesh verbatim (no localization), so the
        // island placed at x=+10 keeps min x ~= 10. The legacy STL path would
        // `localize_stl_triangles` it back to the origin (min x ~= 0).
        let island_triangles = read_stl_triangles_from_zip_entry(&target, "island");
        let island_min_x = island_triangles
            .iter()
            .flat_map(|triangle| triangle.iter())
            .map(|vertex| vertex[0])
            .fold(f32::INFINITY, f32::min);
        assert!(
            island_min_x >= 9.0,
            "sidecar path must preserve authored offset (min x={island_min_x}), not localize"
        );

        // Deterministic replay.
        let first = fs::read(&target).expect("first bytes");
        let replay = root.join("assembly-replay.zip");
        export_multipart_stl_zip_impl(
            &[
                plain_export_part("body", &body_stl),
                plain_export_part("island", &island_stl),
            ],
            replay.to_string_lossy().as_ref(),
            "Assembly".to_string(),
        )
        .expect("replay export");
        assert_eq!(first, fs::read(&replay).expect("replay bytes"));
    }

    #[test]
    fn export_multipart_rejects_malformed_adjacent_sidecar_without_silent_downgrade() {
        let root = temp_export_dir("multipart-sidecar-malformed");
        let (body_vertices, body_triangles) = canonical_tetrahedron();
        let body_stl =
            write_part_with_adjacent_sidecar(&root, "body", &body_vertices, &body_triangles);
        let island_stl =
            write_part_with_adjacent_sidecar(&root, "island", &body_vertices, &body_triangles);
        // Corrupt one sidecar's content digest. A present-but-broken sidecar
        // must fail raw and actionable, never silently fall back to STL.
        tamper_sidecar_digest(&body_stl.with_extension("indexed-mesh.json"));

        let target = root.join("out.3mf");
        let err = export_multipart_3mf_impl(
            &[
                plain_export_part("body", &body_stl),
                plain_export_part("island", &island_stl),
            ],
            target.to_string_lossy().as_ref(),
            "Assembly".to_string(),
        )
        .expect_err("malformed sidecar must fail raw");

        let message = err.to_string();
        assert!(
            message.to_lowercase().contains("sidecar"),
            "error must name the sidecar: {message}"
        );
        assert!(
            !target.exists(),
            "no partial export artifact on sidecar failure"
        );
    }

    #[test]
    fn export_multipart_legacy_part_without_sidecar_keeps_stl_path_behavior() {
        let root = temp_export_dir("multipart-sidecar-legacy-fallback");
        let (body_vertices, body_triangles) = canonical_tetrahedron();
        // `body` ships an adjacent sidecar; `legacy` ships STL only.
        let body_stl =
            write_part_with_adjacent_sidecar(&root, "body", &body_vertices, &body_triangles);
        let legacy_stl = root.join("legacy.stl");
        let soup: Vec<[[f32; 3]; 3]> = body_triangles
            .iter()
            .map(|triangle| {
                let a = body_vertices[triangle[0] as usize];
                let b = body_vertices[triangle[1] as usize];
                let c = body_vertices[triangle[2] as usize];
                [
                    [a[0] as f32, a[1] as f32, a[2] as f32],
                    [b[0] as f32, b[1] as f32, b[2] as f32],
                    [c[0] as f32, c[1] as f32, c[2] as f32],
                ]
            })
            .collect();
        write_binary_stl_triangles_to_path(&legacy_stl, &soup);

        let target = root.join("out.3mf");
        export_multipart_3mf_impl(
            &[
                plain_export_part("body", &body_stl),
                plain_export_part("legacy", &legacy_stl),
            ],
            target.to_string_lossy().as_ref(),
            "Assembly".to_string(),
        )
        .expect("legacy part must keep current STL path behavior");

        // Whole export used the legacy STL path (all-or-nothing routing, no
        // silent per-part representation mixing), so even the sidecar-bearing
        // `body` is STL-reindexed rather than canonical.
        let objects = read_3mf_objects(&target);
        assert_eq!(objects.len(), 2);
        let body_by_name = objects
            .iter()
            .find(|(name, _, _)| name == "body")
            .expect("body object");
        assert_eq!(
            body_by_name.2,
            stl_reindexed_tetrahedron_triangles(),
            "legacy fallback must use STL re-indexing, not the canonical sidecar"
        );
    }

    #[test]
    fn multipart_export_3mf_preserves_distinct_part_geometry() {
        // Regression: two parts with distinct geometry must survive export as two
        // separate objects, not collapse into one. The old suite missed this because
        // write_binary_stl() emits the SAME triangle for every part file.
        let root = temp_export_dir("multipart-distinct-3mf");
        let body_path = root.join("body.stl");
        let lid_path = root.join("lid.stl");
        write_binary_stl_triangles_to_path(
            &body_path,
            &[
                [[0.0, 0.0, 0.0], [50.0, 0.0, 0.0], [0.0, 40.0, 0.0]],
                [[50.0, 0.0, 0.0], [50.0, 40.0, 0.0], [0.0, 40.0, 0.0]],
            ],
        );
        write_binary_stl_triangles_to_path(
            &lid_path,
            &[
                [
                    [-78.0, -48.0, 11.0],
                    [55.0, -48.0, 11.0],
                    [-78.0, 48.0, 11.0],
                ],
                [[55.0, -48.0, 11.0], [55.0, 48.0, 11.0], [-78.0, 48.0, 11.0]],
            ],
        );
        let three_mf_path = root.join("repro.3mf");
        export_multipart_3mf_impl(
            &[
                ExportPartInput {
                    label: "woodlouse_trap_body".to_string(),
                    path: body_path.to_string_lossy().to_string(),
                    object_name: Some("woodlouse_trap_body".to_string()),
                    part_id: Some("part-body".to_string()),
                    display_color: None,
                    placement_frame: None,
                },
                ExportPartInput {
                    label: "woodlouse_trap_slide_lid".to_string(),
                    path: lid_path.to_string_lossy().to_string(),
                    object_name: Some("woodlouse_trap_slide_lid".to_string()),
                    part_id: Some("part-lid".to_string()),
                    display_color: None,
                    placement_frame: None,
                },
            ],
            three_mf_path.to_string_lossy().as_ref(),
            "woodlouse".to_string(),
        )
        .expect("3MF export must succeed for two valid binary STL parts");

        let file = fs::File::open(&three_mf_path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        let mut model_xml = String::new();
        archive
            .by_name("3D/3dmodel.model")
            .unwrap()
            .read_to_string(&mut model_xml)
            .unwrap();

        assert_eq!(
            model_xml.matches("<object ").count(),
            2,
            "expected exactly 2 objects in 3MF\n{}",
            model_xml
        );
        // body spans x=[0,50]; lid (localized) spans x=[0,133]. Distinct geometry
        // means both span widths must appear as vertex coordinates.
        assert!(model_xml.contains(r#"x="50.00000""#), "body geometry lost");
        assert!(model_xml.contains(r#"x="133.00000""#), "lid geometry lost");
    }

    #[test]
    fn multipart_export_rejects_ascii_stl_with_clear_message() {
        // Defense-in-depth: native backends must never feed ASCII STL here once the
        // writer is fixed, but if they do, the error must name ASCII explicitly
        // instead of the cryptic "failed to fill whole buffer".
        let root = temp_export_dir("multipart-ascii-reject");
        let body_path = root.join("body.stl");
        let lid_path = root.join("lid.stl");
        write_binary_stl_triangles_to_path(
            &body_path,
            &[[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]]],
        );
        helper_write_ascii_stl(
            &lid_path,
            &[[
                [-78.0, -48.0, 11.0],
                [55.0, -48.0, 11.0],
                [-78.0, 48.0, 11.0],
            ]],
        );
        let three_mf_path = root.join("ascii.3mf");
        let err = export_multipart_3mf_impl(
            &[
                ExportPartInput {
                    label: "body".to_string(),
                    path: body_path.to_string_lossy().to_string(),
                    object_name: Some("body".to_string()),
                    part_id: Some("part-body".to_string()),
                    display_color: None,
                    placement_frame: None,
                },
                ExportPartInput {
                    label: "lid".to_string(),
                    path: lid_path.to_string_lossy().to_string(),
                    object_name: Some("lid".to_string()),
                    part_id: Some("part-lid".to_string()),
                    display_color: None,
                    placement_frame: None,
                },
            ],
            three_mf_path.to_string_lossy().as_ref(),
            "ascii-model".to_string(),
        )
        .unwrap_err();
        assert!(
            err.to_string().to_lowercase().contains("ascii"),
            "ASCII STL must be rejected with a message naming ASCII, got: {err}"
        );
    }

    #[test]
    fn multipart_stl_zip_rejects_ascii_stl_instead_of_copying_raw_bytes() {
        let root = temp_export_dir("multipart-zip-ascii-reject");
        let body_path = root.join("body.stl");
        let lid_path = root.join("lid.stl");
        let zip_path = root.join("ascii.zip");
        write_binary_stl_triangles_to_path(
            &body_path,
            &[[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]]],
        );
        helper_write_ascii_stl(
            &lid_path,
            &[[
                [-78.0, -48.0, 11.0],
                [55.0, -48.0, 11.0],
                [-78.0, 48.0, 11.0],
            ]],
        );

        let err = export_multipart_stl_zip_impl(
            &[
                ExportPartInput {
                    label: "body".to_string(),
                    path: body_path.to_string_lossy().to_string(),
                    object_name: Some("body".to_string()),
                    part_id: Some("part-body".to_string()),
                    display_color: None,
                    placement_frame: None,
                },
                ExportPartInput {
                    label: "lid".to_string(),
                    path: lid_path.to_string_lossy().to_string(),
                    object_name: Some("lid".to_string()),
                    part_id: Some("part-lid".to_string()),
                    display_color: None,
                    placement_frame: None,
                },
            ],
            zip_path.to_string_lossy().as_ref(),
            "ascii-model".to_string(),
        )
        .unwrap_err();

        assert!(
            err.to_string().to_lowercase().contains("ascii"),
            "ASCII STL must be rejected with a message naming ASCII, got: {err}"
        );
    }

    #[test]
    fn multipart_export_rejects_single_part_models() {
        let root = temp_export_dir("multipart-single");
        let body_path = root.join("body.stl");
        let zip_path = root.join("shade-parts.zip");
        write_binary_stl(&body_path);

        let error = export_multipart_stl_zip_impl(
            &[ExportPartInput {
                label: "Shade Body".to_string(),
                path: body_path.to_string_lossy().to_string(),
                object_name: Some("Body".to_string()),
                part_id: Some("part-body".to_string()),
                display_color: None,
                placement_frame: None,
            }],
            zip_path.to_string_lossy().as_ref(),
            "Bulb Lamp Shade".to_string(),
        )
        .unwrap_err();

        assert!(error.to_string().contains("requires at least two parts"));
    }
}
