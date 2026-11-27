use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Map, Value};

use crate::contracts::{AppError, AppResult};

use super::{
    ensure_target_parent_dir, format_3mf_transform_scalar, indexed_three_mf_mesh,
    read_binary_stl_triangles, xml_escape,
};

pub const PRINT_SETUP_FILE: &str = "print.orca.json";
const PROJECT_SETTINGS: &str = "Metadata/project_settings.config";
const INPUT_LIMIT: u64 = 2 * 1024 * 1024;

pub struct OrcaPrintMesh {
    pub part_id: String,
    pub label: String,
    pub path: PathBuf,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Setup {
    schema_version: u32,
    profile: PathBuf,
    parts: Vec<Part>,
    #[serde(default)]
    layer_events: Vec<LayerEvent>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Part {
    part_id: String,
    filament: usize,
    #[serde(default)]
    offset: [f64; 3],
}

#[derive(Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum LayerEvent {
    Pause { before_layer: u32 },
    FilamentChange { before_layer: u32, filament: usize },
}

struct PlacedMesh<'a> {
    label: &'a str,
    filament: usize,
    offset: [f64; 3],
    vertices: Vec<[f32; 3]>,
    triangles: Vec<[usize; 3]>,
}

fn read_json<T: serde::de::DeserializeOwned>(reader: impl Read, path: &Path) -> AppResult<T> {
    let mut data = Vec::new();
    reader
        .take(INPUT_LIMIT + 1)
        .read_to_end(&mut data)
        .map_err(|error| {
            AppError::persistence(format!("Cannot read '{}': {error}", path.display()))
        })?;
    if data.len() as u64 > INPUT_LIMIT {
        return Err(AppError::validation(format!(
            "'{}' exceeds the 2 MiB print settings limit.",
            path.display()
        )));
    }
    serde_json::from_slice(&data).map_err(|error| {
        AppError::validation(format!(
            "Invalid print settings in '{}': {error}",
            path.display()
        ))
    })
}

fn open_input(path: &Path) -> AppResult<File> {
    File::open(path)
        .map_err(|error| AppError::not_found(format!("Cannot open '{}': {error}", path.display())))
}

fn read_profile(path: &Path) -> AppResult<Map<String, Value>> {
    let mut archive = zip::ZipArchive::new(open_input(path)?).map_err(|error| {
        AppError::validation(format!(
            "Invalid Orca profile '{}': {error}",
            path.display()
        ))
    })?;
    let entry = archive.by_name(PROJECT_SETTINGS).map_err(|_| {
        AppError::validation(format!(
            "'{}' must be an Orca 3MF project with embedded print settings.",
            path.display()
        ))
    })?;
    let mut settings: Map<String, Value> = read_json(entry, path)?;
    for key in [
        "print_host",
        "print_host_apikey",
        "print_host_webui",
        "printhost_apikey",
        "printhost_cafile",
    ] {
        settings.remove(key);
    }
    Ok(settings)
}

fn positive_setting(profile: &Map<String, Value>, key: &str) -> AppResult<f64> {
    profile
        .get(key)
        .and_then(|value| value.as_f64().or_else(|| value.as_str()?.parse().ok()))
        .filter(|number| number.is_finite() && *number > 0.0)
        .ok_or_else(|| AppError::validation(format!("The Orca profile needs a positive {key}.")))
}

fn printable_rectangle(profile: &Map<String, Value>) -> AppResult<([f64; 2], [f64; 2])> {
    let invalid =
        || AppError::validation("Orca print export requires a rectangular printable_area.");
    let points = profile
        .get("printable_area")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    let mut corners = Vec::with_capacity(4);
    if points.len() != 4 {
        return Err(invalid());
    }
    for point in points {
        let (x, y) = point
            .as_str()
            .and_then(|point| point.split_once('x'))
            .ok_or_else(invalid)?;
        let point = [
            x.parse::<f64>().map_err(|_| invalid())?,
            y.parse::<f64>().map_err(|_| invalid())?,
        ];
        if !point.iter().all(|value| value.is_finite()) {
            return Err(invalid());
        }
        corners.push(point);
    }
    let low = std::array::from_fn(|axis| {
        corners
            .iter()
            .map(|point| point[axis])
            .fold(f64::INFINITY, f64::min)
    });
    let high = std::array::from_fn(|axis| {
        corners
            .iter()
            .map(|point| point[axis])
            .fold(f64::NEG_INFINITY, f64::max)
    });
    if low[0] == high[0]
        || low[1] == high[1]
        || ![low, [high[0], low[1]], high, [low[0], high[1]]]
            .iter()
            .all(|point| corners.contains(point))
    {
        return Err(invalid());
    }
    Ok((low, high))
}

fn filament_colors(profile: &Map<String, Value>) -> AppResult<Vec<&str>> {
    let invalid = || {
        AppError::validation(
            "The Orca profile needs matching filament settings and #RRGGBB colours.",
        )
    };
    let colors = profile
        .get("filament_colour")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    if colors.is_empty()
        || profile
            .get("filament_settings_id")
            .and_then(Value::as_array)
            .map(Vec::len)
            != Some(colors.len())
    {
        return Err(invalid());
    }
    colors
        .iter()
        .map(|value| {
            let color = value.as_str().ok_or_else(invalid)?;
            color
                .strip_prefix('#')
                .filter(|digits| {
                    digits.len() == 6 && digits.bytes().all(|digit| digit.is_ascii_hexdigit())
                })
                .map(|_| color)
                .ok_or_else(invalid)
        })
        .collect()
}

fn filament_color<'a>(colors: &[&'a str], filament: usize) -> AppResult<&'a str> {
    filament
        .checked_sub(1)
        .and_then(|index| colors.get(index))
        .copied()
        .ok_or_else(|| {
            AppError::validation(format!(
                "Filament slot {filament} is absent from the Orca profile."
            ))
        })
}

fn place_meshes<'a>(
    parts: &[Part],
    meshes: &'a [OrcaPrintMesh],
    profile: &Map<String, Value>,
    colors: &[&str],
) -> AppResult<(Vec<PlacedMesh<'a>>, f64)> {
    let (bed_low, bed_high) = printable_rectangle(profile)?;
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    let mut placed = Vec::new();
    for selection in parts {
        filament_color(colors, selection.filament)?;
        let mut matching = meshes
            .iter()
            .filter(|mesh| mesh.part_id == selection.part_id);
        let mesh = matching.next().ok_or_else(|| {
            AppError::validation(format!("Rendered part '{}' is missing.", selection.part_id))
        })?;
        if matching.next().is_some() {
            return Err(AppError::validation(format!(
                "Rendered part ID '{}' is ambiguous.",
                selection.part_id
            )));
        }
        let (vertices, triangles) = indexed_three_mf_mesh(read_binary_stl_triangles(&mesh.path)?);
        if vertices.is_empty() {
            return Err(AppError::validation(format!(
                "Rendered part '{}' has an empty mesh.",
                selection.part_id
            )));
        }
        for vertex in &vertices {
            for axis in 0..3 {
                let coordinate = f64::from(vertex[axis]) + selection.offset[axis];
                if !coordinate.is_finite() {
                    return Err(AppError::validation(format!(
                        "Rendered part '{}' has an invalid coordinate.",
                        selection.part_id
                    )));
                }
                low[axis] = low[axis].min(coordinate);
                high[axis] = high[axis].max(coordinate);
            }
        }
        placed.push(PlacedMesh {
            label: &mesh.label,
            filament: selection.filament,
            offset: selection.offset,
            vertices,
            triangles,
        });
    }
    let size = [high[0] - low[0], high[1] - low[1], high[2] - low[2]];
    if size[0] > bed_high[0] - bed_low[0]
        || size[1] > bed_high[1] - bed_low[1]
        || size[2] > positive_setting(profile, "printable_height")?
    {
        return Err(AppError::validation(
            "Selected bodies exceed the Orca profile's printable dimensions.",
        ));
    }
    for mesh in &mut placed {
        for axis in 0..2 {
            mesh.offset[axis] += (bed_low[axis] + bed_high[axis] - low[axis] - high[axis]) / 2.0;
        }
        mesh.offset[2] -= low[2];
    }
    Ok((placed, size[2]))
}

fn layer_events_xml(
    events: &[LayerEvent],
    placed: &[PlacedMesh<'_>],
    model_height: f64,
    profile: &Map<String, Value>,
    colors: &[&str],
) -> AppResult<Option<String>> {
    if events.is_empty() {
        return Ok(None);
    }
    if profile
        .get("print_sequence")
        .and_then(Value::as_str)
        .is_some_and(|sequence| sequence != "by layer")
    {
        return Err(AppError::validation(
            "Orca layer events require the 'by layer' print sequence.",
        ));
    }
    let first = positive_setting(profile, "initial_layer_print_height")?;
    let regular = positive_setting(profile, "layer_height")?;
    let base_filament = placed[0].filament;
    let mut layers = BTreeMap::new();
    for event in events {
        let (layer, event_type, filament, color, gcode) = match *event {
            LayerEvent::Pause { before_layer } => {
                let gcode = profile
                    .get("machine_pause_gcode")
                    .and_then(Value::as_str)
                    .filter(|gcode| !gcode.trim().is_empty())
                    .ok_or_else(|| {
                        AppError::validation("The Orca machine profile has no pause command.")
                    })?;
                (before_layer, 1, base_filament, "", gcode)
            }
            LayerEvent::FilamentChange {
                before_layer,
                filament,
            } => {
                let single_nozzle = profile
                    .get("nozzle_diameter")
                    .and_then(Value::as_array)
                    .is_some_and(|nozzles| nozzles.len() == 1);
                let supports_switch = profile
                    .get("single_extruder_multi_material")
                    .and_then(Value::as_str)
                    == Some("1");
                if !single_nozzle
                    || !supports_switch
                    || placed.iter().any(|mesh| mesh.filament != base_filament)
                {
                    return Err(AppError::validation("Whole-layer filament changes require one nozzle configured for multiple filaments and one starting filament for all bodies."));
                }
                (
                    before_layer,
                    2,
                    filament,
                    filament_color(colors, filament)?,
                    "tool_change",
                )
            }
        };
        let z = first + (f64::from(layer) - 1.0) * regular;
        if layer < 2 || z - regular / 2.0 >= model_height {
            return Err(AppError::validation(format!(
                "Print event at layer {layer} falls outside the printable layers."
            )));
        }
        if layers
            .insert(layer, (z, event_type, filament, color, gcode))
            .is_some()
        {
            return Err(AppError::validation(format!(
                "Only one print event is supported at layer {layer}."
            )));
        }
    }
    let mut xml = String::from("<custom_gcodes_per_layer><plate><plate_info id=\"1\"/>");
    for (z, event_type, filament, color, gcode) in layers.into_values() {
        let _ = write!(
            xml,
            r#"<layer top_z="{}" type="{event_type}" extruder="{filament}" color="{color}" extra="" gcode="{}"/>"#,
            format_3mf_transform_scalar(z),
            xml_escape(gcode)
                .replace('\n', "&#10;")
                .replace('\r', "&#13;"),
        );
    }
    let mode = if colors.len() > 1 {
        "MultiAsSingle"
    } else {
        "SingleExtruder"
    };
    let _ = write!(
        xml,
        r#"<mode value="{mode}"/></plate></custom_gcodes_per_layer>"#
    );
    Ok(Some(xml))
}

fn project_xml(placed: &[PlacedMesh<'_>]) -> (String, String) {
    let mut model = String::from(concat!(
        r#"<?xml version="1.0" encoding="UTF-8"?>"#,
        r#"<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">"#,
        r#"<metadata name="Application">OrcaSlicer-2.4.0-dev</metadata><metadata name="BambuStudio:3mfVersion">1</metadata><resources>"#
    ));
    let mut build = String::from("<build>");
    let mut settings = String::from("<config>");
    let mut plate = String::from(r#"<plate><metadata key="plater_id" value="1"/>"#);
    for (index, mesh) in placed.iter().enumerate() {
        let id = index + 1;
        let label = xml_escape(mesh.label);
        let _ = write!(
            model,
            r#"<object id="{id}" type="model" name="{label}"><mesh><vertices>"#
        );
        for [x, y, z] in &mesh.vertices {
            let _ = write!(model, r#"<vertex x="{x:.5}" y="{y:.5}" z="{z:.5}"/>"#);
        }
        model.push_str("</vertices><triangles>");
        for [a, b, c] in &mesh.triangles {
            let _ = write!(model, r#"<triangle v1="{a}" v2="{b}" v3="{c}"/>"#);
        }
        model.push_str("</triangles></mesh></object>");
        let [x, y, z] = mesh.offset.map(format_3mf_transform_scalar);
        let _ = write!(
            build,
            r#"<item objectid="{id}" transform="1 0 0 0 1 0 0 0 1 {x} {y} {z}" printable="1"/>"#
        );
        let filament = mesh.filament;
        let _ = write!(
            settings,
            concat!(
                r#"<object id="{id}"><metadata key="name" value="{label}"/><metadata key="extruder" value="{filament}"/>"#,
                r#"<part id="{id}" subtype="normal_part"><metadata key="name" value="{label}"/><metadata key="extruder" value="{filament}"/></part></object>"#
            ),
            id = id,
            label = label,
            filament = filament
        );
        let _ = write!(
            plate,
            concat!(
                r#"<model_instance><metadata key="object_id" value="{id}"/>"#,
                r#"<metadata key="instance_id" value="0"/><metadata key="identify_id" value="{id}"/></model_instance>"#
            ),
            id = id
        );
    }
    model.push_str("</resources>");
    model.push_str(&build);
    model.push_str("</build></model>");
    settings.push_str(&plate);
    settings.push_str("</plate></config>");
    (model, settings)
}

/// Write a fresh, editable Orca project using the input meshes and saved setup.
pub fn export_orca_project(
    setup_path: &Path,
    meshes: &[OrcaPrintMesh],
    target: &Path,
) -> AppResult<()> {
    let setup: Setup = read_json(open_input(setup_path)?, setup_path)?;
    if setup.schema_version != 1 || setup.parts.is_empty() || setup.profile.as_os_str().is_empty() {
        return Err(AppError::validation(
            "A print setup needs schemaVersion 1, an Orca profile, and at least one selected part.",
        ));
    }
    let profile_path = setup_path
        .parent()
        .unwrap_or(Path::new(""))
        .join(&setup.profile);
    if let Ok(target_input) = target.canonicalize() {
        let overwrite_input = [setup_path, &profile_path]
            .into_iter()
            .chain(meshes.iter().map(|mesh| mesh.path.as_path()))
            .any(|path| path.canonicalize().is_ok_and(|path| path == target_input));
        if overwrite_input {
            return Err(AppError::validation(
                "Choose a new 3MF path to keep the source setup, profile, and meshes.",
            ));
        }
    }
    let profile = read_profile(&profile_path)?;
    let colors = filament_colors(&profile)?;
    let (placed, model_height) = place_meshes(&setup.parts, meshes, &profile, &colors)?;
    let events = layer_events_xml(
        &setup.layer_events,
        &placed,
        model_height,
        &profile,
        &colors,
    )?;
    let (model, settings) = project_xml(&placed);
    let profile = Value::Object(profile).to_string();
    let mut entries = vec![
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Target="/3D/3dmodel.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/></Relationships>"#,
        ),
        ("3D/3dmodel.model", model.as_str()),
        ("Metadata/model_settings.config", settings.as_str()),
        (PROJECT_SETTINGS, profile.as_str()),
    ];
    if let Some(events) = events.as_deref() {
        entries.push(("Metadata/custom_gcode_per_layer.xml", events));
    }
    ensure_target_parent_dir(target)?;
    let temporary = target.with_extension(format!("{}.part", uuid::Uuid::new_v4()));
    let result = (|| -> Result<(), zip::result::ZipError> {
        let mut zip = zip::ZipWriter::new(
            File::options()
                .write(true)
                .create_new(true)
                .open(&temporary)?,
        );
        let options =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (entry, content) in entries {
            zip.start_file(entry, options)?;
            zip.write_all(content.as_bytes())?;
        }
        zip.finish()?;
        fs::rename(&temporary, target)?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(temporary);
        return Err(AppError::persistence(format!(
            "Failed to write Orca project '{}': {error}",
            target.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
#[path = "orca_project_tests.rs"]
mod tests;
