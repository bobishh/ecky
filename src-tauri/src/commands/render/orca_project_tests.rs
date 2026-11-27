use std::io::{Read, Write};

use serde_json::json;

use super::*;

struct TestJob {
    dir: std::path::PathBuf,
    meshes: Vec<OrcaPrintMesh>,
}

impl TestJob {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("ecky-orca-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let mesh_path = dir.join("body.stl");
        let mut stl = vec![0; 80];
        let vertices = [
            [0.0_f32, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [0.0, 10.0, 0.0],
            [0.0, 0.0, 6.4],
        ];
        let faces = [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];
        stl.extend((faces.len() as u32).to_le_bytes());
        for face in faces {
            stl.extend([0; 12]);
            for vertex in face {
                for coordinate in vertices[vertex] {
                    stl.extend(coordinate.to_le_bytes());
                }
            }
            stl.extend([0; 2]);
        }
        fs::write(&mesh_path, stl).unwrap();
        let profile = json!({
            "printer_model": "Test printer",
            "printer_settings_id": "Test printer 0.6",
            "printable_area": ["0x0", "256x0", "256x256", "0x256"],
            "printable_height": "256",
            "nozzle_diameter": ["0.6"],
            "layer_height": "0.2",
            "initial_layer_print_height": "0.2",
            "filament_colour": ["#F020EE", "#FFFFFF"],
            "filament_settings_id": ["Test PETG", "Test PETG"],
            "single_extruder_multi_material": "1",
            "machine_pause_gcode": "M400 U1"
        });
        let mut zip = zip::ZipWriter::new(fs::File::create(dir.join("profile.3mf")).unwrap());
        zip.start_file(
            "Metadata/project_settings.config",
            zip::write::FileOptions::default(),
        )
        .unwrap();
        zip.write_all(profile.to_string().as_bytes()).unwrap();
        zip.start_file(
            "Metadata/custom_gcode_per_layer.xml",
            zip::write::FileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"original-project-events").unwrap();
        zip.finish().unwrap();
        let meshes = vec![OrcaPrintMesh {
            part_id: "body".into(),
            label: "Body <&>".into(),
            path: mesh_path,
        }];
        Self { dir, meshes }
    }

    fn export(&self, parts: serde_json::Value, events: serde_json::Value) -> AppResult<()> {
        let setup = json!({
            "schemaVersion": 1,
            "profile": "profile.3mf",
            "parts": parts,
            "layerEvents": events
        });
        let setup_path = self.dir.join("print.orca.json");
        fs::write(&setup_path, setup.to_string()).unwrap();
        export_orca_project(&setup_path, &self.meshes, &self.dir.join("job.3mf"))
    }

    fn entry(&self, name: &str) -> String {
        let mut zip =
            zip::ZipArchive::new(fs::File::open(self.dir.join("job.3mf")).unwrap()).unwrap();
        let mut contents = String::new();
        zip.by_name(name)
            .unwrap()
            .read_to_string(&mut contents)
            .unwrap();
        contents
    }
}

impl Drop for TestJob {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn one_body_exports_pause_and_filament_switch_with_profile() {
    let job = TestJob::new();
    job.export(
        json!([{ "partId": "body", "filament": 1 }]),
        json!([
            { "type": "filamentChange", "beforeLayer": 28, "filament": 2 },
            { "type": "pause", "beforeLayer": 18 },
        ]),
    )
    .unwrap();

    let model = job.entry("3D/3dmodel.model");
    assert_eq!(model.matches("<item ").count(), 1);
    assert!(model.contains("Body &lt;&amp;&gt;"));
    let events = job.entry("Metadata/custom_gcode_per_layer.xml");
    assert!(events.contains("top_z=\"3.6\" type=\"1\""));
    assert!(events.contains("top_z=\"5.6\" type=\"2\" extruder=\"2\" color=\"#FFFFFF\""));
    assert!(events.find("3.6").unwrap() < events.find("5.6").unwrap());
    assert!(events.contains("M400 U1"));
    assert!(!events.contains("original-project-events"));
    assert!(job
        .entry("Metadata/project_settings.config")
        .contains("Test printer 0.6"));
}

#[test]
fn multiple_placed_bodies_keep_their_filament_assignments() {
    let job = TestJob::new();
    job.export(
        json!([
            { "partId": "body", "filament": 1 },
            { "partId": "body", "filament": 2, "offset": [20, 0, 0] },
        ]),
        json!([]),
    )
    .unwrap();
    let model = job.entry("3D/3dmodel.model");
    assert!(model.contains("1 0 0 0 1 0 0 0 1 113 123 0"));
    assert!(model.contains("1 0 0 0 1 0 0 0 1 133 123 0"));
    let settings = job.entry("Metadata/model_settings.config");
    assert!(settings.contains("key=\"extruder\" value=\"2\""));
}

#[test]
fn invalid_print_layer_leaves_existing_export_untouched() {
    let job = TestJob::new();
    let output = job.dir.join("job.3mf");
    fs::write(&output, "previous export").unwrap();
    let error = job
        .export(
            json!([{ "partId": "body", "filament": 1 }]),
            json!([{ "type": "pause", "beforeLayer": 90 }]),
        )
        .unwrap_err();
    assert!(error.to_string().contains("layer 90"));
    assert_eq!(fs::read_to_string(output).unwrap(), "previous export");
}
