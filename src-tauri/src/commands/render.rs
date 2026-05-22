use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;

use tauri::{AppHandle, State};

use super::session::write_last_snapshot;
use crate::db;
use crate::freecad;
use crate::models::{
    AppError, AppResult, AppState, ArtifactBundle, BrepHiddenLineProjectionRequest,
    BrepHiddenLineProjectionResponse, DesignOutput, DesignParams, ExportPartInput,
    FreecadLibraryImportRequest, FreecadLibraryItem, FreecadLibrarySearchRequest, InteractionMode,
    MacroDialect, ManifestBounds, ModelManifest, ModelSourceKind, ParamValue, UiField, UiSpec,
};

fn humanize_parameter_key(key: &str) -> String {
    key.split(['_', '-', '.'])
        .filter(|token| !token.is_empty())
        .map(|token| {
            let mut chars = token.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn infer_imported_dimension_value(key: &str, bounds: Option<&ManifestBounds>) -> f64 {
    let Some(bounds) = bounds else {
        return 0.0;
    };

    if key.ends_with("_height") {
        (bounds.z_max - bounds.z_min).max(0.0)
    } else if key.ends_with("_depth") {
        (bounds.y_max - bounds.y_min).max(0.0)
    } else {
        (bounds.x_max - bounds.x_min).max(0.0)
    }
}

fn build_imported_ui_spec(manifest: &ModelManifest) -> UiSpec {
    let mut keys = BTreeSet::new();

    for group in &manifest.parameter_groups {
        if !group.editable {
            continue;
        }
        for key in &group.parameter_keys {
            keys.insert(key.clone());
        }
    }

    for part in &manifest.parts {
        if !part.editable {
            continue;
        }
        for key in &part.parameter_keys {
            keys.insert(key.clone());
        }
    }

    UiSpec {
        fields: keys
            .into_iter()
            .map(|key| UiField::Number {
                label: humanize_parameter_key(&key),
                key,
                min: Some(0.0),
                max: None,
                step: Some(1.0),
                min_from: None,
                max_from: None,
                frozen: false,
            })
            .collect(),
    }
}

fn build_imported_params(
    manifest: &ModelManifest,
    existing_params: &DesignParams,
    ui_spec: &UiSpec,
) -> DesignParams {
    let mut next = DesignParams::new();

    for field in &ui_spec.fields {
        let key = field.key().to_string();
        if let Some(value) = existing_params.get(&key) {
            next.insert(key, value.clone());
            continue;
        }

        let source_part = manifest.parts.iter().find(|part| {
            part.parameter_keys
                .iter()
                .any(|part_key| part_key == field.key())
        });
        next.insert(
            key,
            ParamValue::Number(infer_imported_dimension_value(
                field.key(),
                source_part.and_then(|part| part.bounds.as_ref()),
            )),
        );
    }

    next
}

fn build_imported_output(
    manifest: &ModelManifest,
    existing_output: Option<&DesignOutput>,
) -> DesignOutput {
    let ui_spec = build_imported_ui_spec(manifest);
    let existing_params = existing_output
        .map(|output| output.initial_params.clone())
        .unwrap_or_default();
    let initial_params = build_imported_params(manifest, &existing_params, &ui_spec);
    let title = if manifest.document.document_label.trim().is_empty() {
        if manifest.document.document_name.trim().is_empty() {
            "Imported FreeCAD Model".to_string()
        } else {
            manifest.document.document_name.clone()
        }
    } else {
        manifest.document.document_label.clone()
    };

    DesignOutput {
        title,
        version_name: existing_output
            .map(|output| output.version_name.clone())
            .unwrap_or_else(|| "Imported".to_string()),
        response: "Imported FreeCAD model.".to_string(),
        interaction_mode: InteractionMode::Design,
        macro_code: String::new(),
        macro_dialect: MacroDialect::Legacy,
        engine_kind: crate::models::EngineKind::Freecad,
        source_language: crate::models::SourceLanguage::LegacyPython,
        geometry_backend: crate::models::GeometryBackend::Freecad,
        ui_spec,
        initial_params,
        post_processing: None,
    }
}

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

    for (index, part) in parts.iter().enumerate() {
        zip.start_file(export_entry_name(index, part), options)
            .map_err(|err| {
                AppError::persistence(format!(
                    "Failed to add '{}' to multipart STL archive: {}",
                    export_part_label(part),
                    err
                ))
            })?;
        if let Some(frame) = part.placement_frame.as_ref() {
            let triangles =
                transform_stl_triangles(read_binary_stl_triangles(Path::new(&part.path))?, frame);
            write_binary_stl_triangles(&mut zip, &triangles).map_err(|err| {
                AppError::persistence(format!(
                    "Failed to write transformed '{}' into multipart STL archive: {}",
                    export_part_label(part),
                    err
                ))
            })?;
        } else {
            match read_binary_stl_triangles(Path::new(&part.path)) {
                Ok(triangles) => {
                    let (triangles, _) = localize_stl_triangles(triangles);
                    write_binary_stl_triangles(&mut zip, &triangles).map_err(|err| {
                        AppError::persistence(format!(
                            "Failed to write localized '{}' into multipart STL archive: {}",
                            export_part_label(part),
                            err
                        ))
                    })?;
                }
                Err(_) => {
                    let bytes = fs::read(&part.path).map_err(|err| {
                        AppError::persistence(format!(
                            "Failed to read export part '{}' at '{}': {}",
                            export_part_label(part),
                            part.path,
                            err
                        ))
                    })?;
                    zip.write_all(&bytes).map_err(|err| {
                        AppError::persistence(format!(
                            "Failed to write '{}' into multipart STL archive: {}",
                            export_part_label(part),
                            err
                        ))
                    })?;
                }
            }
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

fn read_f32<R: Read>(reader: &mut R) -> AppResult<f32> {
    let mut bytes = [0u8; 4];
    reader.read_exact(&mut bytes).map_err(|err| {
        AppError::internal(format!(
            "Failed to read STL scalar while exporting multipart model: {}",
            err
        ))
    })?;
    Ok(f32::from_le_bytes(bytes))
}

fn read_vec3<R: Read>(reader: &mut R) -> AppResult<[f32; 3]> {
    Ok([read_f32(reader)?, read_f32(reader)?, read_f32(reader)?])
}

fn read_binary_stl_triangles(path: &Path) -> AppResult<Vec<[[f32; 3]; 3]>> {
    let mut file = File::open(path).map_err(|err| {
        AppError::not_found(format!(
            "Failed to open STL part '{}' for multipart export: {}",
            path.display(),
            err
        ))
    })?;
    let mut header = [0u8; 80];
    file.read_exact(&mut header).map_err(|err| {
        AppError::internal(format!(
            "Failed to read STL header from '{}' while exporting multipart model: {}",
            path.display(),
            err
        ))
    })?;
    let mut count_bytes = [0u8; 4];
    file.read_exact(&mut count_bytes).map_err(|err| {
        AppError::internal(format!(
            "Failed to read STL triangle count from '{}' while exporting multipart model: {}",
            path.display(),
            err
        ))
    })?;
    let triangle_count = u32::from_le_bytes(count_bytes) as usize;
    let mut triangles = Vec::with_capacity(triangle_count);
    for _ in 0..triangle_count {
        let _normal = read_vec3(&mut file)?;
        let a = read_vec3(&mut file)?;
        let b = read_vec3(&mut file)?;
        let c = read_vec3(&mut file)?;
        let mut attr = [0u8; 2];
        file.read_exact(&mut attr).map_err(|err| {
            AppError::internal(format!(
                "Failed to read STL triangle attributes from '{}' while exporting multipart model: {}",
                path.display(),
                err
            ))
        })?;
        triangles.push([a, b, c]);
    }
    Ok(triangles)
}

fn transform_stl_triangles(
    triangles: Vec<[[f32; 3]; 3]>,
    frame: &crate::models::PortFrame,
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

fn transform_stl_vertex(vertex: [f32; 3], frame: &crate::models::PortFrame) -> [f32; 3] {
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
    geometry_backend: Option<crate::models::GeometryBackend>,
    post_processing: Option<crate::contracts::PostProcessingSpec>,
    previous_manifest: Option<ModelManifest>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<ArtifactBundle> {
    render_service::render_model_with_previous_manifest(
        &macro_code,
        &parameters,
        macro_dialect,
        geometry_backend,
        post_processing.as_ref(),
        previous_manifest.as_ref(),
        &state,
        &app,
    )
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn import_fcstd(
    source_path: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<ArtifactBundle> {
    let _guard = state.render_lock.lock().await;
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
pub async fn search_freecad_library(
    request: FreecadLibrarySearchRequest,
    state: State<'_, AppState>,
) -> AppResult<Vec<FreecadLibraryItem>> {
    let configured_roots = {
        let config = state.config.lock().unwrap();
        config.freecad_library_roots.clone()
    };
    crate::freecad_library::search_freecad_library(&request, &configured_roots)
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

    if matches!(extension.as_str(), "stl" | "obj" | "3mf") {
        return crate::freecad_library::import_mesh_from_request(&request, &app);
    }

    let _guard = state.render_lock.lock().await;
    let result = match extension.as_str() {
        "fcstd" => {
            freecad::import_fcstd(source_path, configured_freecad_cmd(&state).as_deref(), &app)
        }
        "step" | "stp" => {
            freecad::import_step(source_path, configured_freecad_cmd(&state).as_deref(), &app)
        }
        other => Err(AppError::validation(format!(
            "FreeCAD library format '{}' is not importable yet.",
            other
        ))),
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
    let _guard = state.render_lock.lock().await;
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
            |err: rusqlite::Error| crate::models::AppError::persistence(err.to_string()),
        )?;
        db::update_message_artifact_bundle(&db, message_id, &next_bundle).map_err(
            |err: rusqlite::Error| crate::models::AppError::persistence(err.to_string()),
        )?;

        let existing_output = db::get_message_output_and_thread(&db, message_id)
            .map_err(|err: rusqlite::Error| crate::models::AppError::persistence(err.to_string()))?
            .map(|(output, _)| output);
        let mut imported_output = build_imported_output(&next_manifest, existing_output.as_ref());
        imported_output.initial_params = parameters.clone();
        db::update_message_output(&db, message_id, &imported_output)
            .map_err(|err| crate::models::AppError::persistence(err.to_string()))?;
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

#[tauri::command]
#[specta::specta]
pub async fn save_model_manifest(
    model_id: String,
    manifest: ModelManifest,
    message_id: Option<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<()> {
    let manifest = crate::model_runtime::write_model_manifest(&app, &model_id, &manifest)?;
    let refreshed_bundle = crate::model_runtime::read_artifact_bundle(&app, &model_id).ok();

    let mut persisted_output: Option<DesignOutput> = None;

    if let Some(message_id) = message_id.as_ref() {
        let db = state.db.lock().await;
        db::update_message_model_manifest(&db, message_id, &manifest).map_err(
            |err: rusqlite::Error| crate::models::AppError::persistence(err.to_string()),
        )?;
        if let Some(bundle) = refreshed_bundle.as_ref() {
            db::update_message_artifact_bundle(&db, message_id, bundle).map_err(
                |err: rusqlite::Error| crate::models::AppError::persistence(err.to_string()),
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
                    crate::models::AppError::persistence(err.to_string())
                })?
                .map(|(output, _)| output);
            let imported_output = build_imported_output(&manifest, existing_output.as_ref());
            db::update_message_output(&db, message_id, &imported_output)
                .map_err(|err| crate::models::AppError::persistence(err.to_string()))?;
            persisted_output = Some(imported_output);
        }
    }

    let snapshot_to_write = {
        let mut last = state.last_snapshot.lock().unwrap();
        let Some(snapshot) = last.as_mut() else {
            return Ok(());
        };

        let snapshot_matches_model = snapshot
            .model_manifest
            .as_ref()
            .map(|current| current.model_id.as_str() == model_id.as_str())
            .unwrap_or(false)
            || snapshot
                .artifact_bundle
                .as_ref()
                .map(|bundle| bundle.model_id.as_str() == model_id.as_str())
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
        write_last_snapshot(&app, Some(snapshot));
    }

    Ok(())
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
        .ok_or_else(|| crate::models::AppError::internal("Invalid mess STL path."))?
        .to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn export_file(source_path: String, target_path: String) -> AppResult<()> {
    std::fs::copy(&source_path, &target_path).map_err(|err| {
        crate::models::AppError::persistence(format!("Failed to export file: {}", err))
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
    use crate::models::{
        Advisory, AdvisoryCondition, AdvisorySeverity, ControlPrimitive, ControlPrimitiveKind,
        ControlView, ControlViewScope, ControlViewSection, ControlViewSource, DocumentMetadata,
        EnrichmentStatus, ManifestEnrichmentState, ParameterGroup, PartBinding, PrimitiveBinding,
        SelectionTarget, SelectionTargetKind, MODEL_RUNTIME_SCHEMA_VERSION,
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
            schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
            model_id: "imported-fcstd-test".to_string(),
            source_kind: ModelSourceKind::ImportedFcstd,
            source_digest: None,
            core_digest: None,
            ast_schema_version: None,
            engine_kind: crate::models::EngineKind::Freecad,
            geometry_backend: crate::models::GeometryBackend::Freecad,
            source_language: crate::models::SourceLanguage::LegacyPython,
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
        let output = build_imported_output(&sample_imported_manifest(), None);

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
                    placement_frame: Some(crate::models::PortFrame {
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
                    placement_frame: Some(crate::models::PortFrame {
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
