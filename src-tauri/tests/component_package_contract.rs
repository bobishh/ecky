use std::fs;
use std::io::{Read, Write};

use base64::Engine as _;
use ecky_cad_lib::commands::component_package as component_package_commands;
use ecky_cad_lib::component_package_runtime::{
    extract_component_package_archive, install_component_package_archive,
    list_installed_component_package_headers, read_component_package_from_archive,
    read_component_package_header_from_archive, read_component_package_manifest,
    resolve_installed_component_source, write_component_package_archive,
    write_component_package_manifest, COMPONENT_PACKAGE_FILE_NAME,
    COMPONENT_PACKAGE_HEADER_FILE_NAME,
};
use ecky_cad_lib::models::{
    component_package_header, validate_component_package, AppState,
    ArtifactBundleComponentPackageRequest, AssemblyComponentRef, AssemblyDefinition, AssemblyMate,
    AssemblyOperation, AssemblyOutput, AssemblyOutputMode, ComponentDefinition,
    ComponentFusionZone, ComponentInterfaceValue, ComponentKeepoutVolume, ComponentPackage,
    ComponentParam, ComponentParamKind, ComponentPort, Config, EngineKind, GeometryBackend,
    KeepoutVolumeKind, MacroDialect, MatePortTypePair, MateTypeDefinition, McpConfig,
    ModelSourceKind, OperationKind, PackageVisibility, PathResolver, PortFrame, PortReference,
    PortTypeDefinition, SketchConstraint, SketchConstraintKind, SketchDefinition, SketchPrimitive,
    SketchPrimitiveKind, SketchView, SourceLanguage, VoiceConfig, COMPONENT_PACKAGE_SCHEMA_VERSION,
};
use zip::ZipArchive;

const COMPONENT_PACKAGE_PAYLOAD_FILE_NAME: &str = "ecky-payload.b64";

struct TempPathResolver {
    root: std::path::PathBuf,
}

impl PathResolver for TempPathResolver {
    fn app_config_dir(&self) -> std::path::PathBuf {
        self.root.join("config")
    }

    fn app_data_dir(&self) -> std::path::PathBuf {
        self.root.join("data")
    }

    fn resource_path(&self, _path: &str) -> Option<std::path::PathBuf> {
        None
    }
}

fn sample_port(port_id: &str, type_id: &str, compatible_with: Vec<String>) -> ComponentPort {
    ComponentPort {
        port_id: port_id.to_string(),
        type_id: type_id.to_string(),
        target_ids: Vec::new(),
        frame: Some(PortFrame::identity()),
        params: Default::default(),
        interfaces: vec!["mechanical_slide".to_string()],
        compatible_with,
        allowed_ops: vec![OperationKind::Mate, OperationKind::Fuse],
    }
}

fn sample_component(
    component_id: &str,
    port_id: &str,
    type_id: &str,
    compatible_with: Vec<String>,
) -> ComponentDefinition {
    ComponentDefinition {
        component_id: component_id.to_string(),
        version: "1.0.0".to_string(),
        display_name: component_id.to_string(),
        source_ref: Some(format!("components/{component_id}/source.ecky")),
        source_language: None,
        geometry_backend: None,
        macro_dialect: None,
        sketches: vec![sample_sketch("front_profile")],
        keepouts: vec![ComponentKeepoutVolume {
            keepout_id: "bottle_clearance".to_string(),
            label: "Bottle Clearance".to_string(),
            kind: KeepoutVolumeKind::Cylinder,
            frame: Some(PortFrame::identity()),
            size: None,
            radius: Some(38.0),
            height: Some(132.0),
        }],
        fusion_zones: vec![ComponentFusionZone {
            zone_id: "rear_spine_patch".to_string(),
            surface_ref: "rear_spine_outer_face".to_string(),
            allowed_ops: vec![OperationKind::Fuse, OperationKind::Blend],
            max_blend_radius: Some(4.0),
            keepout_ids: vec!["bottle_clearance".to_string()],
        }],
        params: vec![ComponentParam {
            key: "mount_spacing".to_string(),
            label: "Mount Spacing".to_string(),
            kind: ComponentParamKind::Number,
            unit: Some("mm".to_string()),
        }],
        ui_spec: ecky_cad_lib::models::UiSpec {
            fields: vec![ecky_cad_lib::models::UiField::Number {
                key: "mount_spacing".to_string(),
                label: "Mount Spacing".to_string(),
                min: Some(40.0),
                max: Some(90.0),
                step: Some(1.0),
                min_from: None,
                max_from: None,
                frozen: false,
            }],
        },
        initial_params: [(
            "mount_spacing".to_string(),
            ecky_cad_lib::models::ParamValue::Number(64.0),
        )]
        .into_iter()
        .collect(),
        ports: vec![sample_port(port_id, type_id, compatible_with)],
    }
}

fn sample_sketch(sketch_id: &str) -> SketchDefinition {
    SketchDefinition {
        sketch_id: sketch_id.to_string(),
        view: SketchView::Front,
        plane: Some(PortFrame::identity()),
        primitives: vec![SketchPrimitive {
            primitive_id: "outer".to_string(),
            kind: SketchPrimitiveKind::Polyline,
            points: vec![[0.0, 0.0], [20.0, 0.0], [20.0, 40.0], [0.0, 40.0]],
            closed: true,
            radius: None,
        }],
        constraints: vec![SketchConstraint {
            constraint_id: "outer_closed".to_string(),
            kind: SketchConstraintKind::Closed,
            target_ids: vec!["outer".to_string()],
            value: None,
        }],
    }
}

fn sample_port_type(type_id: &str, compatible_with: Vec<String>) -> PortTypeDefinition {
    PortTypeDefinition {
        type_id: type_id.to_string(),
        display_name: type_id.to_string(),
        base: Some("dovetail".to_string()),
        interfaces: vec!["mechanical_slide".to_string()],
        compatible_with,
        allowed_ops: vec![OperationKind::Mate, OperationKind::Fuse],
        params: vec![ComponentParam {
            key: "clearance".to_string(),
            label: "Clearance".to_string(),
            kind: ComponentParamKind::Number,
            unit: Some("mm".to_string()),
        }],
    }
}

fn sample_mate_type() -> MateTypeDefinition {
    MateTypeDefinition {
        type_id: "linear_insert".to_string(),
        display_name: "Linear Insert".to_string(),
        allowed_port_type_pairs: vec![MatePortTypePair {
            a_type_id: "mechanical.dovetail.rail.v1".to_string(),
            b_type_id: "mechanical.dovetail.slot.v1".to_string(),
        }],
        params: vec![ComponentParam {
            key: "clearance".to_string(),
            label: "Clearance".to_string(),
            kind: ComponentParamKind::Number,
            unit: Some("mm".to_string()),
        }],
    }
}

fn sample_package() -> ComponentPackage {
    ComponentPackage {
        schema_version: COMPONENT_PACKAGE_SCHEMA_VERSION,
        package_id: "bike.bottle-holder-kit".to_string(),
        version: "0.1.0".to_string(),
        display_name: "Bike Bottle Holder Kit".to_string(),
        visibility: PackageVisibility::Source,
        tags: vec!["bike".to_string(), "bottle-holder".to_string()],
        port_types: vec![
            sample_port_type(
                "mechanical.dovetail.rail.v1",
                vec!["mechanical.dovetail.slot.v1".to_string()],
            ),
            sample_port_type(
                "mechanical.dovetail.slot.v1",
                vec!["mechanical.dovetail.rail.v1".to_string()],
            ),
        ],
        mate_types: vec![sample_mate_type()],
        components: vec![
            sample_component(
                "frame-rail",
                "dovetail_rail",
                "mechanical.dovetail.rail.v1",
                vec!["mechanical.dovetail.slot.v1".to_string()],
            ),
            sample_component(
                "bottle-cage",
                "dovetail_slot",
                "mechanical.dovetail.slot.v1",
                vec!["mechanical.dovetail.rail.v1".to_string()],
            ),
        ],
        assemblies: vec![AssemblyDefinition {
            assembly_id: "bottle-holder".to_string(),
            display_name: "Bottle Holder".to_string(),
            components: vec![
                AssemblyComponentRef {
                    instance_id: "rail".to_string(),
                    component_id: "frame-rail".to_string(),
                },
                AssemblyComponentRef {
                    instance_id: "cage".to_string(),
                    component_id: "bottle-cage".to_string(),
                },
            ],
            mates: vec![AssemblyMate {
                mate_id: "rail-into-cage".to_string(),
                type_id: "linear_insert".to_string(),
                a: PortReference {
                    instance_id: "rail".to_string(),
                    port_id: "dovetail_rail".to_string(),
                },
                b: PortReference {
                    instance_id: "cage".to_string(),
                    port_id: "dovetail_slot".to_string(),
                },
                params: Default::default(),
            }],
            operations: vec![AssemblyOperation {
                operation_id: "fuse-holder".to_string(),
                kind: OperationKind::Fuse,
                target_instance_ids: vec!["rail".to_string(), "cage".to_string()],
                port_refs: vec![],
                params: Default::default(),
            }],
            output: AssemblyOutput {
                mode: AssemblyOutputMode::SeparateParts,
            },
        }],
    }
}

fn write_sample_component_sources(project_dir: &std::path::Path) {
    for component_id in ["frame-rail", "bottle-cage"] {
        let component_dir = project_dir.join("components").join(component_id);
        fs::create_dir_all(&component_dir).expect("component dir");
        fs::write(
            component_dir.join("source.ecky"),
            "(model (part body (box 20 20 10)))",
        )
        .expect("component source");
    }
}

fn write_parametric_component_source(
    project_dir: &std::path::Path,
    component_id: &str,
    source: &str,
) {
    let component_dir = project_dir.join("components").join(component_id);
    fs::create_dir_all(&component_dir).expect("component dir");
    fs::write(component_dir.join("source.ecky"), source).expect("component source");
}

fn test_config() -> Config {
    Config {
        engines: Vec::new(),
        selected_engine_id: String::new(),
        freecad_cmd: String::new(),
        assets: Vec::new(),
        microwave: None,
        voice: VoiceConfig::default(),
        mcp: McpConfig::default(),
        has_seen_onboarding: true,
        connection_type: None,
        default_engine_kind: EngineKind::Freecad,
        default_source_language: SourceLanguage::LegacyPython,
        default_geometry_backend: GeometryBackend::Freecad,
        max_generation_attempts: 3,
        max_verify_attempts: 0,
    }
}

fn test_state(root: &std::path::Path) -> AppState {
    fs::create_dir_all(root).expect("state root");
    let conn = ecky_cad_lib::db::init_db(&root.join("test.db")).expect("test db");
    AppState::new(test_config(), None, conn)
}

fn read_binary_stl_triangles_from_reader<R: Read>(reader: &mut R) -> Vec<[[f32; 3]; 3]> {
    let mut header = [0u8; 80];
    reader.read_exact(&mut header).expect("stl header");
    let mut count_bytes = [0u8; 4];
    reader
        .read_exact(&mut count_bytes)
        .expect("stl triangle count");
    let triangle_count = u32::from_le_bytes(count_bytes) as usize;
    let mut triangles = Vec::with_capacity(triangle_count);
    for _ in 0..triangle_count {
        let mut scalar_bytes = [0u8; 4];
        for _ in 0..3 {
            reader.read_exact(&mut scalar_bytes).expect("stl normal");
        }
        let mut triangle = [[0.0f32; 3]; 3];
        for vertex in &mut triangle {
            for coordinate in vertex.iter_mut() {
                reader
                    .read_exact(&mut scalar_bytes)
                    .expect("stl vertex coordinate");
                *coordinate = f32::from_le_bytes(scalar_bytes);
            }
        }
        let mut attr = [0u8; 2];
        reader.read_exact(&mut attr).expect("stl attrs");
        triangles.push(triangle);
    }
    triangles
}

fn transform_stl_vertex(vertex: [f32; 3], frame: &PortFrame) -> [f32; 3] {
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

fn transform_stl_triangles(triangles: &[[[f32; 3]; 3]], frame: &PortFrame) -> Vec<[[f32; 3]; 3]> {
    triangles
        .iter()
        .map(|triangle| triangle.map(|vertex| transform_stl_vertex(vertex, frame)))
        .collect()
}

fn assert_stl_triangles_approx_eq(actual: &[[[f32; 3]; 3]], expected: &[[[f32; 3]; 3]]) {
    assert_eq!(actual.len(), expected.len());
    for (actual_triangle, expected_triangle) in actual.iter().zip(expected) {
        for (actual_vertex, expected_vertex) in actual_triangle.iter().zip(expected_triangle) {
            for (actual_coordinate, expected_coordinate) in
                actual_vertex.iter().zip(expected_vertex)
            {
                assert!(
                    (actual_coordinate - expected_coordinate).abs() <= 1.0e-4,
                    "actual {:?} expected {:?}",
                    actual_triangle,
                    expected_triangle
                );
            }
        }
    }
}

#[test]
fn package_contract_accepts_component_interfaces_and_assembly_mates() {
    let package = sample_package();

    validate_component_package(&package).expect("valid package should pass");

    let json = serde_json::to_value(&package).expect("package should serialize");
    assert_eq!(
        json["components"][0]["sourceRef"],
        "components/frame-rail/source.ecky"
    );
    assert!(json["components"][0]["componentId"].is_string());
    assert_eq!(json["mateTypes"][0]["typeId"], "linear_insert");
    assert_eq!(
        json["mateTypes"][0]["allowedPortTypePairs"][0]["aTypeId"],
        "mechanical.dovetail.rail.v1"
    );
    assert!(json["assemblies"][0]["mates"][0]["a"]["instanceId"].is_string());
}

#[test]
fn package_contract_rejects_duplicate_mate_type_definitions() {
    let mut package = sample_package();
    package.mate_types.push(sample_mate_type());

    let err = validate_component_package(&package).expect_err("duplicate mate type should fail");

    assert_eq!(
        err.message,
        "component package contains duplicate mate typeId 'linear_insert'."
    );
}

#[test]
fn package_contract_rejects_unknown_assembly_mate_type_when_mate_types_defined() {
    let mut package = sample_package();
    package.assemblies[0].mates[0].type_id = "unknown_mate".to_string();

    let err = validate_component_package(&package).expect_err("unknown mate type should fail");

    assert_eq!(
        err.message,
        "assembly 'bottle-holder' mate 'rail-into-cage' references unknown mate typeId 'unknown_mate'."
    );
}

#[test]
fn package_contract_rejects_mate_type_disallowed_port_pair() {
    let mut package = sample_package();
    package.mate_types[0].allowed_port_type_pairs[0].b_type_id =
        "mechanical.threaded.insert.v1".to_string();

    let err = validate_component_package(&package).expect_err("bad mate pair should fail");

    assert_eq!(
        err.message,
        "assembly 'bottle-holder' mate 'rail-into-cage' typeId 'linear_insert' does not allow port type pair 'mechanical.dovetail.rail.v1' and 'mechanical.dovetail.slot.v1'."
    );
}

#[test]
fn package_contract_rejects_duplicate_component_ports() {
    let mut package = sample_package();
    package.components[0].ports.push(sample_port(
        "dovetail_rail",
        "mechanical.dovetail.rail.v1",
        vec!["mechanical.dovetail.slot.v1".to_string()],
    ));

    let err = validate_component_package(&package).expect_err("duplicate port should fail");

    assert_eq!(
        err.message,
        "component 'frame-rail' contains duplicate portId 'dovetail_rail'."
    );
}

#[test]
fn package_contract_rejects_duplicate_port_type_definitions() {
    let mut package = sample_package();
    package.port_types.push(sample_port_type(
        "mechanical.dovetail.rail.v1",
        vec!["mechanical.dovetail.slot.v1".to_string()],
    ));

    let err = validate_component_package(&package).expect_err("duplicate type should fail");

    assert_eq!(
        err.message,
        "component package contains duplicate port typeId 'mechanical.dovetail.rail.v1'."
    );
}

#[test]
fn package_contract_rejects_incompatible_mated_ports() {
    let mut package = sample_package();
    package.components[0].ports[0].compatible_with.clear();
    package.components[0].ports[0].interfaces.clear();
    package.components[1].ports[0].compatible_with.clear();
    package.components[1].ports[0].interfaces.clear();

    let err = validate_component_package(&package).expect_err("incompatible mate should fail");

    assert_eq!(
        err.message,
        "assembly 'bottle-holder' mate 'rail-into-cage' connects incompatible ports 'rail.dovetail_rail' and 'cage.dovetail_slot'."
    );
}

#[test]
fn package_contract_rejects_sketch_constraints_to_unknown_primitives() {
    let mut package = sample_package();
    package.components[0].sketches[0].constraints[0].target_ids = vec!["missing".to_string()];

    let err = validate_component_package(&package).expect_err("bad sketch target should fail");

    assert_eq!(
        err.message,
        "component 'frame-rail' sketch 'front_profile' constraint 'outer_closed' references unknown primitiveId 'missing'."
    );
}

#[test]
fn package_contract_rejects_fusion_zones_with_unknown_keepouts() {
    let mut package = sample_package();
    package.components[0].fusion_zones[0].keepout_ids = vec!["missing_keepout".to_string()];

    let err = validate_component_package(&package).expect_err("bad keepout ref should fail");

    assert_eq!(
        err.message,
        "component 'frame-rail' fusion zone 'rear_spine_patch' references unknown keepoutId 'missing_keepout'."
    );
}

#[test]
fn package_contract_rejects_operations_with_unknown_instances() {
    let mut package = sample_package();
    package.assemblies[0].operations[0].target_instance_ids = vec!["missing".to_string()];

    let err = validate_component_package(&package).expect_err("bad operation target should fail");

    assert_eq!(
        err.message,
        "assembly 'bottle-holder' operation 'fuse-holder' references unknown instanceId 'missing'."
    );
}

#[test]
fn package_contract_rejects_mates_to_unknown_ports() {
    let mut package = sample_package();
    package.assemblies[0].mates[0].b.port_id = "missing_slot".to_string();

    let err = validate_component_package(&package).expect_err("unknown mate port should fail");

    assert_eq!(
        err.message,
        "assembly 'bottle-holder' mate 'rail-into-cage' references unknown portId 'missing_slot' on instance 'cage'."
    );
}

#[test]
fn package_archive_round_trips_valid_header_without_exposing_payload_shape() {
    let package = sample_package();
    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let project_dir = temp_root.join("project");
    write_sample_component_sources(&project_dir);

    let manifest_path =
        write_component_package_manifest(&project_dir, &package).expect("write manifest");
    assert_eq!(
        manifest_path.file_name().unwrap(),
        COMPONENT_PACKAGE_FILE_NAME
    );
    let read_back = read_component_package_manifest(&project_dir).expect("read manifest");
    assert_eq!(read_back.package_id, "bike.bottle-holder-kit");

    let archive_path = temp_root.join("bike-kit.ecky");
    write_component_package_archive(&project_dir, &archive_path).expect("write archive");
    let archived = read_component_package_from_archive(&archive_path).expect("read archive");

    assert_eq!(archived.package_id, "bike.bottle-holder-kit");
    assert_eq!(archived.components.len(), 2);

    let header = read_component_package_header_from_archive(&archive_path).expect("read header");
    let header_json = serde_json::to_value(&header).expect("header json");
    assert_eq!(header_json["packageId"], "bike.bottle-holder-kit");
    assert_eq!(
        header_json["portTypes"][0]["typeId"],
        "mechanical.dovetail.rail.v1"
    );
    assert_eq!(header_json["components"][0]["componentId"], "frame-rail");
    assert!(header_json["components"][0].get("sourceRef").is_none());

    let archive_file = fs::File::open(&archive_path).expect("archive open");
    let mut archive = zip::ZipArchive::new(archive_file).expect("archive parse");
    assert!(archive.by_name(COMPONENT_PACKAGE_HEADER_FILE_NAME).is_ok());
    assert!(archive.by_name(COMPONENT_PACKAGE_FILE_NAME).is_err());
    let mut payload_entry = archive
        .by_name(COMPONENT_PACKAGE_PAYLOAD_FILE_NAME)
        .expect("payload entry");
    let mut payload_raw = String::new();
    payload_entry
        .read_to_string(&mut payload_raw)
        .expect("payload read");
    assert!(!payload_raw.contains("sourceRef"));
    assert!(base64::engine::general_purpose::STANDARD
        .decode(payload_raw.trim())
        .is_ok());

    fs::remove_dir_all(temp_root).ok();
}

#[test]
fn package_archive_reads_legacy_plain_manifest_archives() {
    let package = sample_package();
    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-legacy-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir_all(&temp_root).expect("temp dir");
    let archive_path = temp_root.join("legacy.ecky");
    let archive_file = fs::File::create(&archive_path).expect("archive create");
    let mut writer = zip::ZipWriter::new(archive_file);
    writer
        .start_file(
            COMPONENT_PACKAGE_FILE_NAME,
            zip::write::FileOptions::default(),
        )
        .expect("manifest entry");
    writer
        .write_all(
            serde_json::to_string_pretty(&package)
                .expect("manifest json")
                .as_bytes(),
        )
        .expect("manifest write");
    writer
        .start_file(
            "components/frame-rail/source.ecky",
            zip::write::FileOptions::default(),
        )
        .expect("source entry");
    writer.write_all(b"(model)").expect("source write");
    writer.finish().expect("archive finish");

    let archived = read_component_package_from_archive(&archive_path).expect("read legacy");
    let target_dir = temp_root.join("legacy-extracted");
    let extracted = extract_component_package_archive(&archive_path, &target_dir).expect("extract");

    assert_eq!(archived.package_id, "bike.bottle-holder-kit");
    assert_eq!(extracted.package_id, "bike.bottle-holder-kit");
    assert!(target_dir.join(COMPONENT_PACKAGE_FILE_NAME).exists());
    assert!(target_dir
        .join("components/frame-rail/source.ecky")
        .exists());

    fs::remove_dir_all(temp_root).ok();
}

#[test]
fn package_archive_header_can_be_read_without_full_manifest_payload() {
    let package = sample_package();
    let header = component_package_header(&package).expect("header");
    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-header-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir_all(&temp_root).expect("temp dir");
    let archive_path = temp_root.join("header-only.ecky");
    let archive_file = fs::File::create(&archive_path).expect("archive create");
    let mut writer = zip::ZipWriter::new(archive_file);
    writer
        .start_file(
            COMPONENT_PACKAGE_HEADER_FILE_NAME,
            zip::write::FileOptions::default(),
        )
        .expect("header entry");
    writer
        .write_all(
            serde_json::to_string_pretty(&header)
                .expect("header json")
                .as_bytes(),
        )
        .expect("header write");
    writer.finish().expect("archive finish");

    let read_back = read_component_package_header_from_archive(&archive_path).expect("read header");

    assert_eq!(read_back.package_id, "bike.bottle-holder-kit");
    assert_eq!(read_back.components.len(), 2);

    fs::remove_dir_all(temp_root).ok();
}

#[test]
fn package_archive_extracts_payload_safely() {
    let package = sample_package();
    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-extract-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let project_dir = temp_root.join("project");
    write_sample_component_sources(&project_dir);
    write_component_package_manifest(&project_dir, &package).expect("write manifest");
    let archive_path = temp_root.join("bike-kit.ecky");
    write_component_package_archive(&project_dir, &archive_path).expect("write archive");
    let target_dir = temp_root.join("extracted");

    let extracted = extract_component_package_archive(&archive_path, &target_dir).expect("extract");

    assert_eq!(extracted.package_id, "bike.bottle-holder-kit");
    assert!(target_dir.join(COMPONENT_PACKAGE_FILE_NAME).exists());
    assert!(target_dir
        .join("components/frame-rail/source.ecky")
        .exists());

    fs::remove_dir_all(temp_root).ok();
}

#[test]
fn package_archive_extract_rejects_traversal_entries() {
    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-extract-bad-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir_all(&temp_root).expect("temp dir");
    let archive_path = temp_root.join("bad.ecky");
    let archive_file = fs::File::create(&archive_path).expect("archive create");
    let mut writer = zip::ZipWriter::new(archive_file);
    writer
        .start_file("../evil.txt", zip::write::FileOptions::default())
        .expect("bad entry");
    writer.write_all(b"evil").expect("bad write");
    writer.finish().expect("archive finish");

    let err = extract_component_package_archive(&archive_path, &temp_root.join("out"))
        .expect_err("traversal must fail");

    assert_eq!(
        err.message,
        "Component package archive entry '../evil.txt' is not safe to extract."
    );

    fs::remove_dir_all(temp_root).ok();
}

#[test]
fn package_archive_installs_into_local_library_and_lists_header() {
    let package = sample_package();
    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-install-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };
    let project_dir = temp_root.join("project");
    write_sample_component_sources(&project_dir);
    write_component_package_manifest(&project_dir, &package).expect("write manifest");
    let archive_path = temp_root.join("bike-kit.ecky");
    write_component_package_archive(&project_dir, &archive_path).expect("write archive");

    let installed =
        install_component_package_archive(&resolver, &archive_path).expect("install package");
    let headers =
        list_installed_component_package_headers(&resolver).expect("list installed headers");

    assert_eq!(installed.header.package_id, "bike.bottle-holder-kit");
    assert!(std::path::Path::new(&installed.package_dir).exists());
    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0].package_id, "bike.bottle-holder-kit");

    let resolved = resolve_installed_component_source(
        &resolver,
        "bike.bottle-holder-kit",
        "0.1.0",
        "frame-rail",
    )
    .expect("resolve installed component source");
    assert_eq!(resolved.package_id, "bike.bottle-holder-kit");
    assert_eq!(resolved.component.component_id, "frame-rail");
    assert_eq!(resolved.component.ports[0].port_id, "dovetail_rail");
    assert_eq!(resolved.component.ui_spec.fields.len(), 1);
    assert_eq!(
        resolved.component.initial_params.get("mount_spacing"),
        Some(&ecky_cad_lib::models::ParamValue::Number(64.0))
    );
    assert!(std::path::Path::new(&resolved.source_path).is_file());
    assert!(resolved
        .source_path
        .ends_with("components/frame-rail/source.ecky"));

    fs::remove_dir_all(temp_root).ok();
}

#[test]
fn package_header_exposes_interface_not_source_refs() {
    let package = sample_package();

    let header = component_package_header(&package).expect("header");
    let header_json = serde_json::to_value(&header).expect("header json");

    assert_eq!(header.components.len(), 2);
    assert_eq!(header.port_types.len(), 2);
    assert_eq!(header.components[0].ports[0].port_id, "dovetail_rail");
    assert_eq!(header.components[0].ui_spec.fields.len(), 1);
    assert_eq!(
        header.components[0].initial_params.get("mount_spacing"),
        Some(&ecky_cad_lib::models::ParamValue::Number(64.0))
    );
    assert_eq!(header.assemblies[0].mate_count, 1);
    assert_eq!(header.assemblies[0].operation_count, 1);
    assert!(header_json["components"][0].get("sourceRef").is_none());
    assert!(header_json["components"][0].get("sourceLanguage").is_none());
    assert!(header_json["components"][0]
        .get("geometryBackend")
        .is_none());
    assert!(header_json["components"][0].get("macroDialect").is_none());
    assert!(header_json["components"][0].get("sketches").is_none());
    assert!(header_json["assemblies"][0].get("mates").is_none());
}

#[tokio::test]
async fn package_commands_expose_archive_write_and_header_read() {
    let package = sample_package();
    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-command-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let project_dir = temp_root.join("project");
    write_sample_component_sources(&project_dir);
    write_component_package_manifest(&project_dir, &package).expect("write manifest");
    let archive_path = temp_root.join("bike-kit.ecky");

    component_package_commands::write_component_package_archive(
        project_dir.to_string_lossy().to_string(),
        archive_path.to_string_lossy().to_string(),
    )
    .await
    .expect("command write archive");

    let header = component_package_commands::read_component_package_header_from_archive(
        archive_path.to_string_lossy().to_string(),
    )
    .await
    .expect("command read header");

    assert_eq!(header.package_id, "bike.bottle-holder-kit");
    assert_eq!(header.components.len(), 2);

    let extracted = temp_root.join("command-extracted");
    let command_package = component_package_commands::extract_component_package_archive(
        archive_path.to_string_lossy().to_string(),
        extracted.to_string_lossy().to_string(),
    )
    .await
    .expect("command extract");

    assert_eq!(command_package.package_id, "bike.bottle-holder-kit");

    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };
    let installed = component_package_commands::install_component_package_archive_for_app(
        &resolver,
        archive_path.to_string_lossy().to_string(),
    )
    .await
    .expect("command install");
    let headers =
        component_package_commands::list_installed_component_package_headers_for_app(&resolver)
            .await
            .expect("command list installed");
    let resolved = component_package_commands::resolve_installed_component_source_for_app(
        &resolver,
        "bike.bottle-holder-kit".to_string(),
        "0.1.0".to_string(),
        "frame-rail".to_string(),
    )
    .await
    .expect("command resolve installed component");
    let assembly = component_package_commands::resolve_installed_component_assembly_for_app(
        &resolver,
        "bike.bottle-holder-kit".to_string(),
        "0.1.0".to_string(),
        "bottle-holder".to_string(),
    )
    .await
    .expect("command resolve installed assembly");

    assert_eq!(installed.header.package_id, "bike.bottle-holder-kit");
    assert_eq!(headers.len(), 1);
    assert_eq!(resolved.component.component_id, "frame-rail");
    assert_eq!(assembly.assembly.assembly_id, "bottle-holder");
    assert_eq!(assembly.components.len(), 2);
    assert!(resolved
        .source_path
        .ends_with("components/frame-rail/source.ecky"));

    fs::remove_dir_all(temp_root).ok();
}

#[tokio::test]
async fn render_installed_component_source_renders_installed_ecky_component() {
    let mut package = sample_package();
    package.components[0].source_language = Some(SourceLanguage::EckyIrV0);
    package.components[0].geometry_backend = Some(GeometryBackend::Freecad);
    package.components[0].macro_dialect = Some(MacroDialect::EckyIrV0);

    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-render-ecky-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };
    let state = test_state(&temp_root);
    if !ecky_cad_lib::services::render::is_freecad_available(&state) {
        fs::remove_dir_all(temp_root).ok();
        return;
    }

    let project_dir = temp_root.join("project");
    write_sample_component_sources(&project_dir);
    write_component_package_manifest(&project_dir, &package).expect("write manifest");
    let archive_path = temp_root.join("bike-kit.ecky");
    write_component_package_archive(&project_dir, &archive_path).expect("write archive");
    component_package_commands::install_component_package_archive_for_app(
        &resolver,
        archive_path.to_string_lossy().to_string(),
    )
    .await
    .expect("install package");

    let rendered = component_package_commands::render_installed_component_source_for_app(
        &resolver,
        &state,
        "bike.bottle-holder-kit".to_string(),
        "0.1.0".to_string(),
        "frame-rail".to_string(),
        Default::default(),
    )
    .await
    .expect("render installed component");

    assert_eq!(
        rendered.installed_source.component.source_ref.as_deref(),
        Some("components/frame-rail/source.ecky")
    );
    assert_eq!(
        rendered.installed_source.component.source_language,
        Some(SourceLanguage::EckyIrV0)
    );
    assert_eq!(
        rendered.installed_source.component.geometry_backend,
        Some(GeometryBackend::Freecad)
    );
    assert_eq!(
        rendered.installed_source.component.macro_dialect,
        Some(MacroDialect::EckyIrV0)
    );
    assert!(rendered
        .installed_source
        .source_path
        .ends_with("components/frame-rail/source.ecky"));
    assert_eq!(
        rendered.artifact_bundle.source_language,
        SourceLanguage::EckyIrV0
    );
    assert_eq!(
        rendered.artifact_bundle.geometry_backend,
        GeometryBackend::Freecad
    );
    assert!(rendered
        .model_manifest
        .document
        .source_path
        .as_deref()
        .is_some_and(|path| path.ends_with("/source.ecky")));
    assert!(rendered
        .artifact_bundle
        .macro_path
        .as_deref()
        .is_some_and(|path| path.ends_with("/source.ecky")));

    fs::remove_dir_all(temp_root).ok();
}

#[tokio::test]
async fn resolve_installed_component_controls_merges_package_initial_params_with_overrides() {
    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-controls-default-params-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };

    let project_dir = temp_root.join("project");
    write_parametric_component_source(
        &project_dir,
        "parametric-body",
        r#"(model
            (params
              (number width 11 :label "Width"))
            (part body (box width 20 10)))"#,
    );
    write_component_package_manifest(
        &project_dir,
        &ComponentPackage {
            schema_version: COMPONENT_PACKAGE_SCHEMA_VERSION,
            package_id: "generated.parametric-body-kit".to_string(),
            version: "0.1.0".to_string(),
            display_name: "Generated Parametric Body Kit".to_string(),
            visibility: PackageVisibility::Source,
            tags: vec!["generated".to_string()],
            port_types: Vec::new(),
            mate_types: Vec::new(),
            components: vec![ComponentDefinition {
                component_id: "parametric-body".to_string(),
                version: "1.0.0".to_string(),
                display_name: "Parametric Body".to_string(),
                source_ref: Some("components/parametric-body/source.ecky".to_string()),
                source_language: Some(SourceLanguage::EckyIrV0),
                geometry_backend: Some(GeometryBackend::Build123d),
                macro_dialect: Some(MacroDialect::EckyIrV0),
                sketches: Vec::new(),
                keepouts: Vec::new(),
                fusion_zones: Vec::new(),
                params: vec![ComponentParam {
                    key: "width".to_string(),
                    label: "Width".to_string(),
                    kind: ComponentParamKind::Number,
                    unit: None,
                }],
                ui_spec: ecky_cad_lib::models::UiSpec {
                    fields: vec![ecky_cad_lib::models::UiField::Number {
                        key: "width".to_string(),
                        label: "Width".to_string(),
                        min: Some(1.0),
                        max: Some(100.0),
                        step: Some(1.0),
                        min_from: None,
                        max_from: None,
                        frozen: false,
                    }],
                },
                initial_params: [(
                    "width".to_string(),
                    ecky_cad_lib::models::ParamValue::Number(42.0),
                )]
                .into_iter()
                .collect(),
                ports: Vec::new(),
            }],
            assemblies: Vec::new(),
        },
    )
    .expect("write package manifest");
    let archive_path = temp_root.join("parametric-body-kit.ecky");
    write_component_package_archive(&project_dir, &archive_path).expect("write archive");
    component_package_commands::install_component_package_archive_for_app(
        &resolver,
        archive_path.to_string_lossy().to_string(),
    )
    .await
    .expect("install package");

    let resolved_default =
        component_package_commands::resolve_installed_component_controls_for_app(
            &resolver,
            "generated.parametric-body-kit".to_string(),
            "0.1.0".to_string(),
            "parametric-body".to_string(),
            Default::default(),
        )
        .await
        .expect("resolve default controls");
    assert_eq!(
        resolved_default.parameters.get("width"),
        Some(&ecky_cad_lib::models::ParamValue::Number(42.0))
    );

    let resolved_override =
        component_package_commands::resolve_installed_component_controls_for_app(
            &resolver,
            "generated.parametric-body-kit".to_string(),
            "0.1.0".to_string(),
            "parametric-body".to_string(),
            [(
                "width".to_string(),
                ecky_cad_lib::models::ParamValue::Number(24.0),
            )]
            .into_iter()
            .collect(),
        )
        .await
        .expect("resolve override controls");
    assert_eq!(
        resolved_override.parameters.get("width"),
        Some(&ecky_cad_lib::models::ParamValue::Number(24.0))
    );
    assert_eq!(
        resolved_override
            .installed_source
            .component
            .ui_spec
            .fields
            .len(),
        1
    );

    fs::remove_dir_all(temp_root).ok();
}

#[tokio::test]
async fn resolve_installed_component_assembly_controls_merge_instance_initial_params() {
    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-assembly-controls-default-params-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };

    let project_dir = temp_root.join("project");
    let package = sample_package();
    write_sample_component_sources(&project_dir);
    write_component_package_manifest(&project_dir, &package).expect("write manifest");
    let archive_path = temp_root.join("bike-kit.ecky");
    write_component_package_archive(&project_dir, &archive_path).expect("write archive");

    component_package_commands::install_component_package_archive_for_app(
        &resolver,
        archive_path.to_string_lossy().to_string(),
    )
    .await
    .expect("install package");

    let assembly =
        component_package_commands::resolve_installed_component_assembly_controls_for_app(
            &resolver,
            "bike.bottle-holder-kit".to_string(),
            "0.1.0".to_string(),
            "bottle-holder".to_string(),
            [(
                "rail".to_string(),
                [(
                    "mount_spacing".to_string(),
                    ecky_cad_lib::models::ParamValue::Number(72.0),
                )]
                .into_iter()
                .collect(),
            )]
            .into_iter()
            .collect(),
        )
        .await
        .expect("resolve installed assembly controls");

    assert!(assembly.mates_solved);
    assert_eq!(assembly.mate_results.len(), 1);

    let rail = assembly
        .components
        .iter()
        .find(|component| component.instance_id == "rail")
        .expect("rail instance");
    let cage = assembly
        .components
        .iter()
        .find(|component| component.instance_id == "cage")
        .expect("cage instance");

    assert_eq!(
        rail.parameters.get("mount_spacing"),
        Some(&ecky_cad_lib::models::ParamValue::Number(72.0))
    );
    assert_eq!(
        cage.parameters.get("mount_spacing"),
        Some(&ecky_cad_lib::models::ParamValue::Number(64.0))
    );
    assert_eq!(rail.placement_frame, Some(PortFrame::identity()));
    assert_eq!(cage.placement_frame, Some(PortFrame::identity()));
    assert_eq!(rail.installed_source.component.ui_spec.fields.len(), 1);

    fs::remove_dir_all(temp_root).ok();
}

#[tokio::test]
async fn render_installed_component_source_merges_package_initial_params_with_overrides() {
    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-render-default-params-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };
    let state = test_state(&temp_root);
    let capability = ecky_cad_lib::runtime_capabilities::probe_build123d_runtime(&resolver);
    if !capability.available {
        fs::remove_dir_all(temp_root).ok();
        return;
    }

    let project_dir = temp_root.join("project");
    write_parametric_component_source(
        &project_dir,
        "parametric-body",
        r#"(model
            (params
              (number width 11 :label "Width"))
            (part body (box width 20 10)))"#,
    );
    write_component_package_manifest(
        &project_dir,
        &ComponentPackage {
            schema_version: COMPONENT_PACKAGE_SCHEMA_VERSION,
            package_id: "generated.parametric-body-kit".to_string(),
            version: "0.1.0".to_string(),
            display_name: "Generated Parametric Body Kit".to_string(),
            visibility: PackageVisibility::Source,
            tags: vec!["generated".to_string()],
            port_types: Vec::new(),
            mate_types: Vec::new(),
            components: vec![ComponentDefinition {
                component_id: "parametric-body".to_string(),
                version: "1.0.0".to_string(),
                display_name: "Parametric Body".to_string(),
                source_ref: Some("components/parametric-body/source.ecky".to_string()),
                source_language: Some(SourceLanguage::EckyIrV0),
                geometry_backend: Some(GeometryBackend::Build123d),
                macro_dialect: Some(MacroDialect::EckyIrV0),
                sketches: Vec::new(),
                keepouts: Vec::new(),
                fusion_zones: Vec::new(),
                params: vec![ComponentParam {
                    key: "width".to_string(),
                    label: "Width".to_string(),
                    kind: ComponentParamKind::Number,
                    unit: None,
                }],
                ui_spec: ecky_cad_lib::models::UiSpec {
                    fields: vec![ecky_cad_lib::models::UiField::Number {
                        key: "width".to_string(),
                        label: "Width".to_string(),
                        min: Some(1.0),
                        max: Some(100.0),
                        step: Some(1.0),
                        min_from: None,
                        max_from: None,
                        frozen: false,
                    }],
                },
                initial_params: [(
                    "width".to_string(),
                    ecky_cad_lib::models::ParamValue::Number(42.0),
                )]
                .into_iter()
                .collect(),
                ports: Vec::new(),
            }],
            assemblies: Vec::new(),
        },
    )
    .expect("write package manifest");
    let archive_path = temp_root.join("parametric-body-kit.ecky");
    write_component_package_archive(&project_dir, &archive_path).expect("write archive");
    component_package_commands::install_component_package_archive_for_app(
        &resolver,
        archive_path.to_string_lossy().to_string(),
    )
    .await
    .expect("install package");

    let rendered_default = component_package_commands::render_installed_component_source_for_app(
        &resolver,
        &state,
        "generated.parametric-body-kit".to_string(),
        "0.1.0".to_string(),
        "parametric-body".to_string(),
        Default::default(),
    )
    .await
    .expect("render default component");
    assert_eq!(
        rendered_default.parameters.get("width"),
        Some(&ecky_cad_lib::models::ParamValue::Number(42.0))
    );
    let default_bounds = rendered_default.model_manifest.parts[0]
        .bounds
        .as_ref()
        .expect("default bounds");
    assert!((default_bounds.x_max - default_bounds.x_min - 42.0).abs() < 1.0e-6);

    let rendered_override = component_package_commands::render_installed_component_source_for_app(
        &resolver,
        &state,
        "generated.parametric-body-kit".to_string(),
        "0.1.0".to_string(),
        "parametric-body".to_string(),
        [(
            "width".to_string(),
            ecky_cad_lib::models::ParamValue::Number(24.0),
        )]
        .into_iter()
        .collect(),
    )
    .await
    .expect("render override component");
    assert_eq!(
        rendered_override.parameters.get("width"),
        Some(&ecky_cad_lib::models::ParamValue::Number(24.0))
    );
    let override_bounds = rendered_override.model_manifest.parts[0]
        .bounds
        .as_ref()
        .expect("override bounds");
    assert!((override_bounds.x_max - override_bounds.x_min - 24.0).abs() < 1.0e-6);

    fs::remove_dir_all(temp_root).ok();
}

#[tokio::test]
async fn render_installed_component_source_imports_installed_step_component() {
    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-render-step-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };
    let state = test_state(&temp_root);
    if !ecky_cad_lib::services::render::is_freecad_available(&state) {
        fs::remove_dir_all(temp_root).ok();
        return;
    }

    let source = include_str!("fixtures/cad/surface/canonical_cup.ecky");
    let generated = ecky_cad_lib::freecad::render_model_with_sources(
        &ecky_cad_lib::ecky_ir::lower_to_freecad(source).expect("lower"),
        Some(source),
        &Default::default(),
        None,
        &resolver,
        SourceLanguage::EckyIrV0,
    )
    .expect("generate step fixture");
    let step_export = generated
        .export_artifacts
        .iter()
        .find(|artifact| artifact.format == "step")
        .expect("step export");

    let mut package = sample_package();
    package.components[0].source_ref = Some("components/frame-rail/source.step".to_string());
    package.components[0].source_language = None;
    package.components[0].geometry_backend = None;
    package.components[0].macro_dialect = None;

    let project_dir = temp_root.join("project");
    write_sample_component_sources(&project_dir);
    let step_component_dir = project_dir.join("components").join("frame-rail");
    fs::copy(&step_export.path, step_component_dir.join("source.step")).expect("copy step");
    write_component_package_manifest(&project_dir, &package).expect("write manifest");
    let archive_path = temp_root.join("bike-kit-step.ecky");
    write_component_package_archive(&project_dir, &archive_path).expect("write archive");
    component_package_commands::install_component_package_archive_for_app(
        &resolver,
        archive_path.to_string_lossy().to_string(),
    )
    .await
    .expect("install package");

    let rendered = component_package_commands::render_installed_component_source_for_app(
        &resolver,
        &state,
        "bike.bottle-holder-kit".to_string(),
        "0.1.0".to_string(),
        "frame-rail".to_string(),
        Default::default(),
    )
    .await
    .expect("render installed step component");

    assert!(rendered
        .installed_source
        .source_path
        .ends_with("components/frame-rail/source.step"));
    assert_eq!(
        rendered.artifact_bundle.source_kind,
        ModelSourceKind::ImportedStep
    );
    assert_eq!(
        rendered.model_manifest.source_kind,
        ModelSourceKind::ImportedStep
    );
    assert_eq!(
        rendered.model_manifest.document.source_path.as_deref(),
        Some(rendered.installed_source.source_path.as_str())
    );
    assert!(std::path::Path::new(&rendered.artifact_bundle.fcstd_path).is_file());

    fs::remove_dir_all(temp_root).ok();
}

#[tokio::test]
async fn runtime_bundle_component_package_project_preserves_exact_source_and_rerenders() {
    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-runtime-bundle-project-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };
    let state = test_state(&temp_root);
    let capability = ecky_cad_lib::runtime_capabilities::probe_build123d_runtime(&resolver);
    if !capability.available {
        fs::remove_dir_all(temp_root).ok();
        return;
    }

    let bundle = ecky_cad_lib::services::render::render_model(
        r#"(model
            (part body
              (sampled-radial-loft
                (theta z fz)
                :height 40
                :z-steps 6
                :theta-steps 24
                :radius (+ 20 (* 2 (sin (+ (* theta 6) (* fz 3.141592653589793)))))
                :z-map (+ z (* fz 2)))))"#,
        &Default::default(),
        Some(MacroDialect::EckyIrV0),
        Some(GeometryBackend::Build123d),
        None,
        &state,
        &resolver,
    )
    .await
    .expect("render exact source bundle");

    let project_dir = temp_root.join("project");
    let package = component_package_commands::write_artifact_bundle_component_package_project(
        project_dir.to_string_lossy().to_string(),
        ArtifactBundleComponentPackageRequest {
            package_id: "generated.sampled-shell-kit".to_string(),
            version: "0.1.0".to_string(),
            display_name: "Generated Sampled Shell Kit".to_string(),
            tags: vec!["generated".to_string(), "sampled".to_string()],
            component_id: "sampled-body".to_string(),
            component_version: "1.0.0".to_string(),
            component_display_name: "Sampled Body".to_string(),
            source_ref: None,
            artifact_bundle: bundle,
            port_types: vec![sample_port_type(
                "mechanical.plane.mount.v1",
                vec!["mechanical.plane.mount.v1".to_string()],
            )],
            params: vec![ComponentParam {
                key: "amp".to_string(),
                label: "Amp".to_string(),
                kind: ComponentParamKind::Number,
                unit: Some("mm".to_string()),
            }],
            ui_spec: ecky_cad_lib::models::UiSpec::default(),
            initial_params: Default::default(),
            ports: vec![ComponentPort {
                port_id: "mount".to_string(),
                type_id: "mechanical.plane.mount.v1".to_string(),
                target_ids: Vec::new(),
                frame: Some(PortFrame::identity()),
                params: Default::default(),
                interfaces: vec!["mechanical_mount".to_string()],
                compatible_with: vec!["mechanical.plane.mount.v1".to_string()],
                allowed_ops: vec![OperationKind::Mate],
            }],
        },
    )
    .await
    .expect("write runtime bundle component package project");

    assert_eq!(package.package_id, "generated.sampled-shell-kit");
    assert_eq!(
        package.components[0].source_ref.as_deref(),
        Some("components/sampled-body/source.ecky")
    );
    assert_eq!(
        package.components[0].source_language,
        Some(SourceLanguage::EckyIrV0)
    );
    assert_eq!(
        package.components[0].geometry_backend,
        Some(GeometryBackend::Build123d)
    );
    assert_eq!(
        package.components[0].macro_dialect,
        Some(MacroDialect::EckyIrV0)
    );
    assert!(project_dir
        .join("components")
        .join("sampled-body")
        .join("source.ecky")
        .is_file());

    let archive_path = temp_root.join("generated-sampled.ecky");
    write_component_package_archive(&project_dir, &archive_path).expect("write archive");
    component_package_commands::install_component_package_archive_for_app(
        &resolver,
        archive_path.to_string_lossy().to_string(),
    )
    .await
    .expect("install package");

    let rendered = component_package_commands::render_installed_component_source_for_app(
        &resolver,
        &state,
        "generated.sampled-shell-kit".to_string(),
        "0.1.0".to_string(),
        "sampled-body".to_string(),
        Default::default(),
    )
    .await
    .expect("render installed packaged runtime source");

    assert_eq!(
        rendered.installed_source.component.source_ref.as_deref(),
        Some("components/sampled-body/source.ecky")
    );
    assert_eq!(
        rendered.installed_source.component.source_language,
        Some(SourceLanguage::EckyIrV0)
    );
    assert_eq!(
        rendered.installed_source.component.geometry_backend,
        Some(GeometryBackend::Build123d)
    );
    assert!(rendered
        .installed_source
        .source_path
        .ends_with("components/sampled-body/source.ecky"));
    assert_eq!(
        rendered.artifact_bundle.source_language,
        SourceLanguage::EckyIrV0
    );
    assert_eq!(
        rendered.artifact_bundle.geometry_backend,
        GeometryBackend::Build123d
    );

    fs::remove_dir_all(temp_root).ok();
}

#[tokio::test]
async fn runtime_bundle_component_package_project_derives_component_params_from_source() {
    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-runtime-bundle-derived-params-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };
    let state = test_state(&temp_root);
    let capability = ecky_cad_lib::runtime_capabilities::probe_build123d_runtime(&resolver);
    if !capability.available {
        fs::remove_dir_all(temp_root).ok();
        return;
    }

    let bundle = ecky_cad_lib::services::render::render_model(
        r#"(model
            (params
              (number amp 2 :min 0 :max 5 :step 0.5 :label "Amplitude")
              (select profile "bulb" :label "Profile"
                :options (("Bulb" "bulb") ("Lantern" "lantern")))
              (toggle vents #t :label "Vents")
              (image reference "" :label "Reference"))
            (part body
              (sampled-radial-loft
                (theta z fz)
                :height 40
                :z-steps 6
                :theta-steps 24
                :radius (+ 20 (* amp (sin (+ (* theta 6) (* fz 3.141592653589793)))))
                :z-map (+ z (* fz 2)))))"#,
        &Default::default(),
        Some(MacroDialect::EckyIrV0),
        Some(GeometryBackend::Build123d),
        None,
        &state,
        &resolver,
    )
    .await
    .expect("render exact source bundle with params");

    let package = component_package_commands::write_artifact_bundle_component_package_project(
        temp_root.join("project").to_string_lossy().to_string(),
        ArtifactBundleComponentPackageRequest {
            package_id: "generated.derived-params-kit".to_string(),
            version: "0.1.0".to_string(),
            display_name: "Generated Derived Params Kit".to_string(),
            tags: vec!["generated".to_string(), "parametric".to_string()],
            component_id: "sampled-body".to_string(),
            component_version: "1.0.0".to_string(),
            component_display_name: "Sampled Body".to_string(),
            source_ref: None,
            artifact_bundle: bundle,
            port_types: vec![sample_port_type(
                "mechanical.plane.mount.v1",
                vec!["mechanical.plane.mount.v1".to_string()],
            )],
            params: Vec::new(),
            ui_spec: ecky_cad_lib::models::UiSpec::default(),
            initial_params: Default::default(),
            ports: vec![ComponentPort {
                port_id: "mount".to_string(),
                type_id: "mechanical.plane.mount.v1".to_string(),
                target_ids: Vec::new(),
                frame: Some(PortFrame::identity()),
                params: Default::default(),
                interfaces: vec!["mechanical_mount".to_string()],
                compatible_with: vec!["mechanical.plane.mount.v1".to_string()],
                allowed_ops: vec![OperationKind::Mate],
            }],
        },
    )
    .await
    .expect("write runtime bundle component package project with derived params");

    assert_eq!(
        package.components[0].params,
        vec![
            ComponentParam {
                key: "amp".to_string(),
                label: "Amplitude".to_string(),
                kind: ComponentParamKind::Number,
                unit: None,
            },
            ComponentParam {
                key: "profile".to_string(),
                label: "Profile".to_string(),
                kind: ComponentParamKind::Choice,
                unit: None,
            },
            ComponentParam {
                key: "vents".to_string(),
                label: "Vents".to_string(),
                kind: ComponentParamKind::Boolean,
                unit: None,
            },
            ComponentParam {
                key: "reference".to_string(),
                label: "Reference".to_string(),
                kind: ComponentParamKind::Text,
                unit: None,
            },
        ]
    );
    assert_eq!(package.components[0].ui_spec.fields.len(), 4);
    assert_eq!(package.components[0].ui_spec.fields[0].key(), "amp");
    assert_eq!(
        package.components[0].initial_params.get("amp"),
        Some(&ecky_cad_lib::models::ParamValue::Number(2.0))
    );
    assert_eq!(
        package.components[0].initial_params.get("profile"),
        Some(&ecky_cad_lib::models::ParamValue::String(
            "bulb".to_string()
        ))
    );
    assert_eq!(
        package.components[0].initial_params.get("vents"),
        Some(&ecky_cad_lib::models::ParamValue::Boolean(true))
    );

    fs::remove_dir_all(temp_root).ok();
}

#[tokio::test]
async fn runtime_bundle_component_package_project_allows_zero_port_geometry_components() {
    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-runtime-bundle-zero-port-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };
    let state = test_state(&temp_root);
    let capability = ecky_cad_lib::runtime_capabilities::probe_build123d_runtime(&resolver);
    if !capability.available {
        fs::remove_dir_all(temp_root).ok();
        return;
    }

    let bundle = ecky_cad_lib::services::render::render_model(
        r#"(model
            (params
              (number amp 2 :min 0 :max 5 :step 0.5 :label "Amplitude"))
            (part body
              (sampled-radial-loft
                (theta z fz)
                :height 40
                :z-steps 6
                :theta-steps 24
                :radius (+ 20 (* amp (sin (+ (* theta 6) (* fz 3.141592653589793)))))
                :z-map (+ z (* fz 2)))))"#,
        &Default::default(),
        Some(MacroDialect::EckyIrV0),
        Some(GeometryBackend::Build123d),
        None,
        &state,
        &resolver,
    )
    .await
    .expect("render exact source bundle");

    let package = component_package_commands::write_artifact_bundle_component_package_project(
        temp_root.join("project").to_string_lossy().to_string(),
        ArtifactBundleComponentPackageRequest {
            package_id: "generated.zero-port-geometry".to_string(),
            version: "0.1.0".to_string(),
            display_name: "Generated Zero Port Geometry".to_string(),
            tags: vec!["generated".to_string(), "decorative".to_string()],
            component_id: "sampled-body".to_string(),
            component_version: "1.0.0".to_string(),
            component_display_name: "Sampled Body".to_string(),
            source_ref: None,
            artifact_bundle: bundle,
            port_types: Vec::new(),
            params: Vec::new(),
            ui_spec: ecky_cad_lib::models::UiSpec::default(),
            initial_params: Default::default(),
            ports: Vec::new(),
        },
    )
    .await
    .expect("write zero-port runtime bundle package");

    assert!(package.port_types.is_empty());
    assert!(package.components[0].ports.is_empty());
    assert_eq!(package.components[0].params.len(), 1);
    assert_eq!(package.components[0].params[0].key, "amp");
    assert_eq!(package.components[0].ui_spec.fields.len(), 1);
    assert_eq!(
        package.components[0].initial_params.get("amp"),
        Some(&ecky_cad_lib::models::ParamValue::Number(2.0))
    );

    let archive_path = temp_root.join("generated-zero-port.ecky");
    write_component_package_archive(&temp_root.join("project"), &archive_path)
        .expect("write archive");
    component_package_commands::install_component_package_archive_for_app(
        &resolver,
        archive_path.to_string_lossy().to_string(),
    )
    .await
    .expect("install zero-port package");

    let resolved = component_package_commands::resolve_installed_component_source_for_app(
        &resolver,
        "generated.zero-port-geometry".to_string(),
        "0.1.0".to_string(),
        "sampled-body".to_string(),
    )
    .await
    .expect("resolve installed zero-port component");
    assert!(resolved.component.ports.is_empty());
    assert_eq!(resolved.component.params.len(), 1);
    assert_eq!(resolved.component.params[0].key, "amp");
    assert_eq!(resolved.component.ui_spec.fields.len(), 1);
    assert_eq!(
        resolved.component.initial_params.get("amp"),
        Some(&ecky_cad_lib::models::ParamValue::Number(2.0))
    );

    let rendered = component_package_commands::render_installed_component_source_for_app(
        &resolver,
        &state,
        "generated.zero-port-geometry".to_string(),
        "0.1.0".to_string(),
        "sampled-body".to_string(),
        Default::default(),
    )
    .await
    .expect("render installed zero-port component");
    assert!(rendered.installed_source.component.ports.is_empty());
    assert_eq!(rendered.installed_source.component.params.len(), 1);
    assert_eq!(rendered.installed_source.component.params[0].key, "amp");
    assert_eq!(rendered.installed_source.component.ui_spec.fields.len(), 1);
    assert_eq!(
        rendered
            .installed_source
            .component
            .initial_params
            .get("amp"),
        Some(&ecky_cad_lib::models::ParamValue::Number(2.0))
    );

    fs::remove_dir_all(temp_root).ok();
}

#[tokio::test]
async fn runtime_bundle_component_package_project_preserves_explicit_ui_contract_for_step_source() {
    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-runtime-bundle-explicit-ui-contract-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir_all(&temp_root).expect("temp root");
    let step_path = temp_root.join("source.step");
    fs::write(&step_path, "ISO-10303-21;\nEND-ISO-10303-21;\n").expect("write step");
    let manifest_path = temp_root.join("manifest.json");
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&ecky_cad_lib::models::ModelManifest {
            schema_version: ecky_cad_lib::models::MODEL_RUNTIME_SCHEMA_VERSION,
            model_id: "step-model".to_string(),
            source_kind: ModelSourceKind::ImportedStep,
            engine_kind: EngineKind::Freecad,
            source_language: SourceLanguage::LegacyPython,
            geometry_backend: GeometryBackend::Freecad,
            document: ecky_cad_lib::models::DocumentMetadata {
                document_name: "Step".to_string(),
                document_label: "Step".to_string(),
                source_path: Some(step_path.to_string_lossy().to_string()),
                object_count: 1,
                warnings: Vec::new(),
            },
            parts: Vec::new(),
            parameter_groups: Vec::new(),
            control_primitives: Vec::new(),
            control_relations: Vec::new(),
            control_views: Vec::new(),
            advisories: Vec::new(),
            selection_targets: Vec::new(),
            measurement_annotations: Vec::new(),
            warnings: Vec::new(),
            enrichment_state: ecky_cad_lib::models::ManifestEnrichmentState {
                status: ecky_cad_lib::models::EnrichmentStatus::None,
                proposals: Vec::new(),
            },
        })
        .expect("serialize manifest"),
    )
    .expect("write manifest");
    let preview_path = temp_root.join("preview.stl");
    fs::write(&preview_path, "solid fake\nendsolid fake\n").expect("write preview");

    let ui_spec = ecky_cad_lib::models::UiSpec {
        fields: vec![
            ecky_cad_lib::models::UiField::Number {
                key: "diameter".to_string(),
                label: "Diameter".to_string(),
                min: Some(60.0),
                max: Some(180.0),
                step: Some(1.0),
                min_from: None,
                max_from: None,
                frozen: false,
            },
            ecky_cad_lib::models::UiField::Select {
                key: "profile".to_string(),
                label: "Profile".to_string(),
                options: vec![
                    ecky_cad_lib::models::SelectOption {
                        label: "Bulb".to_string(),
                        value: ecky_cad_lib::models::SelectValue::String("bulb".to_string()),
                    },
                    ecky_cad_lib::models::SelectOption {
                        label: "Lantern".to_string(),
                        value: ecky_cad_lib::models::SelectValue::String("lantern".to_string()),
                    },
                ],
                frozen: false,
            },
        ],
    };
    let initial_params: ecky_cad_lib::models::DesignParams = [
        (
            "diameter".to_string(),
            ecky_cad_lib::models::ParamValue::Number(120.0),
        ),
        (
            "profile".to_string(),
            ecky_cad_lib::models::ParamValue::String("bulb".to_string()),
        ),
    ]
    .into_iter()
    .collect();

    let project_dir = temp_root.join("project");
    let package = component_package_commands::write_artifact_bundle_component_package_project(
        project_dir.to_string_lossy().to_string(),
        ArtifactBundleComponentPackageRequest {
            package_id: "generated.step-ui-contract".to_string(),
            version: "0.1.0".to_string(),
            display_name: "Generated Step UI Contract".to_string(),
            tags: vec!["generated".to_string(), "step".to_string()],
            component_id: "step-body".to_string(),
            component_version: "1.0.0".to_string(),
            component_display_name: "Step Body".to_string(),
            source_ref: None,
            artifact_bundle: ecky_cad_lib::models::ArtifactBundle {
                schema_version: ecky_cad_lib::models::MODEL_RUNTIME_SCHEMA_VERSION,
                model_id: "step-model".to_string(),
                source_kind: ModelSourceKind::ImportedStep,
                engine_kind: EngineKind::Freecad,
                source_language: SourceLanguage::LegacyPython,
                geometry_backend: GeometryBackend::Freecad,
                content_hash: "step-hash".to_string(),
                artifact_version: 1,
                fcstd_path: String::new(),
                manifest_path: manifest_path.to_string_lossy().to_string(),
                macro_path: None,
                preview_stl_path: preview_path.to_string_lossy().to_string(),
                viewer_assets: Vec::new(),
                edge_targets: Vec::new(),
                face_targets: Vec::new(),
                callout_anchors: Vec::new(),
                measurement_guides: Vec::new(),
                export_artifacts: vec![ecky_cad_lib::models::ExportArtifact {
                    label: "source.step".to_string(),
                    format: "step".to_string(),
                    path: step_path.to_string_lossy().to_string(),
                    role: "primary".to_string(),
                }],
            },
            port_types: Vec::new(),
            params: Vec::new(),
            ui_spec: ui_spec.clone(),
            initial_params: initial_params.clone(),
            ports: Vec::new(),
        },
    )
    .await
    .expect("write step package with explicit ui contract");

    assert_eq!(package.components[0].params.len(), 2);
    assert_eq!(package.components[0].ui_spec, ui_spec);
    assert_eq!(package.components[0].initial_params, initial_params);

    let archive_path = temp_root.join("generated-step-ui-contract.ecky");
    write_component_package_archive(&project_dir, &archive_path).expect("write archive");
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };
    component_package_commands::install_component_package_archive_for_app(
        &resolver,
        archive_path.to_string_lossy().to_string(),
    )
    .await
    .expect("install explicit ui contract package");

    let resolved = component_package_commands::resolve_installed_component_source_for_app(
        &resolver,
        "generated.step-ui-contract".to_string(),
        "0.1.0".to_string(),
        "step-body".to_string(),
    )
    .await
    .expect("resolve installed explicit ui contract package");

    assert_eq!(resolved.component.ui_spec, ui_spec);
    assert_eq!(resolved.component.initial_params, initial_params);
    assert_eq!(resolved.component.params.len(), 2);

    fs::remove_dir_all(temp_root).ok();
}

#[tokio::test]
async fn resolve_installed_component_source_backfills_params_from_source_when_manifest_params_empty(
) {
    let temp_root = std::env::temp_dir().join(format!(
        "ecky-installed-source-derived-params-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let project_dir = temp_root.join("project");
    let source_dir = project_dir.join("components").join("sampled-body");
    fs::create_dir_all(&source_dir).expect("source dir");
    fs::write(
        source_dir.join("source.ecky"),
        r#"(model
            (params
              (number amp 2 :label "Amplitude")
              (toggle vents #t :label "Vents"))
            (part body
              (sampled-radial-loft
                (theta z fz)
                :height 40
                :z-steps 6
                :theta-steps 24
                :radius (+ 20 (* amp (sin (+ (* theta 6) (* fz 3.141592653589793)))))
                :z-map (+ z (* fz 2)))))"#,
    )
    .expect("write source");
    write_component_package_manifest(
        &project_dir,
        &ComponentPackage {
            schema_version: COMPONENT_PACKAGE_SCHEMA_VERSION,
            package_id: "generated.legacy-parametric".to_string(),
            version: "0.1.0".to_string(),
            display_name: "Generated Legacy Parametric".to_string(),
            visibility: PackageVisibility::Source,
            tags: vec!["generated".to_string()],
            port_types: Vec::new(),
            mate_types: Vec::new(),
            components: vec![ComponentDefinition {
                component_id: "sampled-body".to_string(),
                version: "1.0.0".to_string(),
                display_name: "Sampled Body".to_string(),
                source_ref: Some("components/sampled-body/source.ecky".to_string()),
                source_language: Some(SourceLanguage::EckyIrV0),
                geometry_backend: Some(GeometryBackend::Build123d),
                macro_dialect: Some(MacroDialect::EckyIrV0),
                sketches: Vec::new(),
                keepouts: Vec::new(),
                fusion_zones: Vec::new(),
                params: Vec::new(),
                ui_spec: ecky_cad_lib::models::UiSpec::default(),
                initial_params: Default::default(),
                ports: Vec::new(),
            }],
            assemblies: Vec::new(),
        },
    )
    .expect("write package manifest");

    let archive_path = temp_root.join("generated-legacy-parametric.ecky");
    write_component_package_archive(&project_dir, &archive_path).expect("write archive");
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };
    component_package_commands::install_component_package_archive_for_app(
        &resolver,
        archive_path.to_string_lossy().to_string(),
    )
    .await
    .expect("install package");

    let resolved = component_package_commands::resolve_installed_component_source_for_app(
        &resolver,
        "generated.legacy-parametric".to_string(),
        "0.1.0".to_string(),
        "sampled-body".to_string(),
    )
    .await
    .expect("resolve installed component");

    assert_eq!(
        resolved.component.params,
        vec![
            ComponentParam {
                key: "amp".to_string(),
                label: "Amplitude".to_string(),
                kind: ComponentParamKind::Number,
                unit: None,
            },
            ComponentParam {
                key: "vents".to_string(),
                label: "Vents".to_string(),
                kind: ComponentParamKind::Boolean,
                unit: None,
            },
        ]
    );
    assert_eq!(resolved.component.ui_spec.fields.len(), 2);
    assert_eq!(
        resolved.component.initial_params.get("amp"),
        Some(&ecky_cad_lib::models::ParamValue::Number(2.0))
    );
    assert_eq!(
        resolved.component.initial_params.get("vents"),
        Some(&ecky_cad_lib::models::ParamValue::Boolean(true))
    );

    fs::remove_dir_all(temp_root).ok();
}

#[tokio::test]
async fn runtime_bundle_component_package_project_rejects_unknown_runtime_target_id() {
    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-runtime-bundle-bad-target-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir_all(&temp_root).expect("temp root");
    let source_path = temp_root.join("source.ecky");
    fs::write(&source_path, "(model (part body (box 10 10 10)))").expect("write source");
    let manifest_path = temp_root.join("manifest.json");
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&ecky_cad_lib::models::ModelManifest {
            schema_version: ecky_cad_lib::models::MODEL_RUNTIME_SCHEMA_VERSION,
            model_id: "fake-model".to_string(),
            source_kind: ModelSourceKind::Generated,
            engine_kind: EngineKind::EckyIrV0,
            source_language: SourceLanguage::EckyIrV0,
            geometry_backend: GeometryBackend::Build123d,
            document: ecky_cad_lib::models::DocumentMetadata {
                document_name: "Fake".to_string(),
                document_label: "Fake".to_string(),
                source_path: Some(source_path.to_string_lossy().to_string()),
                object_count: 1,
                warnings: Vec::new(),
            },
            parts: Vec::new(),
            parameter_groups: Vec::new(),
            control_primitives: Vec::new(),
            control_relations: Vec::new(),
            control_views: Vec::new(),
            advisories: Vec::new(),
            selection_targets: Vec::new(),
            measurement_annotations: Vec::new(),
            warnings: Vec::new(),
            enrichment_state: ecky_cad_lib::models::ManifestEnrichmentState {
                status: ecky_cad_lib::models::EnrichmentStatus::None,
                proposals: Vec::new(),
            },
        })
        .expect("serialize manifest"),
    )
    .expect("write manifest");
    let preview_path = temp_root.join("preview.stl");
    fs::write(&preview_path, "solid fake\nendsolid fake\n").expect("write preview");

    let err = component_package_commands::write_artifact_bundle_component_package_project(
        temp_root.join("project").to_string_lossy().to_string(),
        ArtifactBundleComponentPackageRequest {
            package_id: "generated.bad-target".to_string(),
            version: "0.1.0".to_string(),
            display_name: "Generated Bad Target".to_string(),
            tags: Vec::new(),
            component_id: "body".to_string(),
            component_version: "1.0.0".to_string(),
            component_display_name: "Body".to_string(),
            source_ref: None,
            artifact_bundle: ecky_cad_lib::models::ArtifactBundle {
                schema_version: ecky_cad_lib::models::MODEL_RUNTIME_SCHEMA_VERSION,
                model_id: "fake-model".to_string(),
                source_kind: ModelSourceKind::Generated,
                engine_kind: EngineKind::EckyIrV0,
                source_language: SourceLanguage::EckyIrV0,
                geometry_backend: GeometryBackend::Build123d,
                content_hash: "fake-hash".to_string(),
                artifact_version: 1,
                fcstd_path: String::new(),
                manifest_path: manifest_path.to_string_lossy().to_string(),
                macro_path: Some(source_path.to_string_lossy().to_string()),
                preview_stl_path: preview_path.to_string_lossy().to_string(),
                viewer_assets: Vec::new(),
                edge_targets: Vec::new(),
                face_targets: Vec::new(),
                callout_anchors: Vec::new(),
                measurement_guides: Vec::new(),
                export_artifacts: Vec::new(),
            },
            port_types: vec![sample_port_type(
                "mechanical.plane.mount.v1",
                vec!["mechanical.plane.mount.v1".to_string()],
            )],
            params: Vec::new(),
            ui_spec: ecky_cad_lib::models::UiSpec::default(),
            initial_params: Default::default(),
            ports: vec![ComponentPort {
                port_id: "mount".to_string(),
                type_id: "mechanical.plane.mount.v1".to_string(),
                target_ids: vec!["missing-target".to_string()],
                frame: Some(PortFrame::identity()),
                params: Default::default(),
                interfaces: vec!["mechanical_mount".to_string()],
                compatible_with: vec!["mechanical.plane.mount.v1".to_string()],
                allowed_ops: vec![OperationKind::Mate],
            }],
        },
    )
    .await
    .expect_err("unknown runtime target should fail");

    assert!(err
        .to_string()
        .contains("unknown runtime targetId 'missing-target'"));

    fs::remove_dir_all(temp_root).ok();
}

#[tokio::test]
async fn direct_runtime_bundle_component_package_project_preserves_topology_target_ids() {
    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-direct-runtime-targets-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };
    let state = test_state(&temp_root);
    let capability = ecky_cad_lib::runtime_capabilities::probe_direct_occt_runtime(&resolver);
    if !capability.available {
        fs::remove_dir_all(temp_root).ok();
        return;
    }

    let bundle = ecky_cad_lib::services::render::render_model(
        r#"(model
            (part body
              (sampled-radial-loft
                (theta z fz)
                :height 40
                :z-steps 6
                :theta-steps 24
                :radius (+ 20 (* 2 (sin (+ (* theta 6) (* fz 3.141592653589793)))))
                :z-map (+ z (* fz 2)))))"#,
        &Default::default(),
        Some(MacroDialect::EckyIrV0),
        Some(GeometryBackend::EckyRust),
        None,
        &state,
        &resolver,
    )
    .await
    .expect("render direct exact source bundle");
    let chosen_target_id = bundle
        .face_targets
        .first()
        .map(|target| target.target_id.clone())
        .or_else(|| {
            bundle
                .edge_targets
                .first()
                .map(|target| target.target_id.clone())
        })
        .expect("direct runtime topology target");

    let project_dir = temp_root.join("project");
    let package = component_package_commands::write_artifact_bundle_component_package_project(
        project_dir.to_string_lossy().to_string(),
        ArtifactBundleComponentPackageRequest {
            package_id: "generated.direct-target-kit".to_string(),
            version: "0.1.0".to_string(),
            display_name: "Generated Direct Target Kit".to_string(),
            tags: vec!["generated".to_string(), "direct".to_string()],
            component_id: "sampled-body".to_string(),
            component_version: "1.0.0".to_string(),
            component_display_name: "Sampled Body".to_string(),
            source_ref: None,
            artifact_bundle: bundle,
            port_types: vec![sample_port_type(
                "mechanical.patch.mate.v1",
                vec!["mechanical.patch.mate.v1".to_string()],
            )],
            params: Vec::new(),
            ui_spec: ecky_cad_lib::models::UiSpec::default(),
            initial_params: Default::default(),
            ports: vec![ComponentPort {
                port_id: "patch".to_string(),
                type_id: "mechanical.patch.mate.v1".to_string(),
                target_ids: vec![chosen_target_id.clone()],
                frame: Some(PortFrame::identity()),
                params: Default::default(),
                interfaces: vec!["surface_patch".to_string()],
                compatible_with: vec!["mechanical.patch.mate.v1".to_string()],
                allowed_ops: vec![OperationKind::Mate],
            }],
        },
    )
    .await
    .expect("write direct runtime bundle component package project");

    assert_eq!(
        package.components[0].ports[0].target_ids,
        vec![chosen_target_id.clone()]
    );

    let archive_path = temp_root.join("generated-direct-targets.ecky");
    write_component_package_archive(&project_dir, &archive_path).expect("write archive");
    component_package_commands::install_component_package_archive_for_app(
        &resolver,
        archive_path.to_string_lossy().to_string(),
    )
    .await
    .expect("install package");

    let resolved = component_package_commands::resolve_installed_component_source_for_app(
        &resolver,
        "generated.direct-target-kit".to_string(),
        "0.1.0".to_string(),
        "sampled-body".to_string(),
    )
    .await
    .expect("resolve installed component");
    assert_eq!(
        resolved.component.ports[0].target_ids,
        vec![chosen_target_id.clone()]
    );

    let rendered = component_package_commands::render_installed_component_source_for_app(
        &resolver,
        &state,
        "generated.direct-target-kit".to_string(),
        "0.1.0".to_string(),
        "sampled-body".to_string(),
        Default::default(),
    )
    .await
    .expect("render installed packaged direct source");

    assert_eq!(
        rendered.installed_source.component.ports[0].target_ids,
        vec![chosen_target_id]
    );
    assert!(
        !rendered.artifact_bundle.face_targets.is_empty()
            || !rendered.artifact_bundle.edge_targets.is_empty()
    );

    fs::remove_dir_all(temp_root).ok();
}

#[tokio::test]
async fn render_installed_component_source_rejects_missing_runtime_target_ids() {
    let mut package = sample_package();
    package.components[0].source_language = Some(SourceLanguage::EckyIrV0);
    package.components[0].geometry_backend = Some(GeometryBackend::Freecad);
    package.components[0].macro_dialect = Some(MacroDialect::EckyIrV0);
    package.components[0].ports[0].target_ids = vec!["missing-topology-target".to_string()];

    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-render-target-fail-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };
    let state = test_state(&temp_root);
    if !ecky_cad_lib::services::render::is_freecad_available(&state) {
        fs::remove_dir_all(temp_root).ok();
        return;
    }

    let project_dir = temp_root.join("project");
    write_sample_component_sources(&project_dir);
    write_component_package_manifest(&project_dir, &package).expect("write manifest");
    let archive_path = temp_root.join("bike-kit.ecky");
    write_component_package_archive(&project_dir, &archive_path).expect("write archive");
    component_package_commands::install_component_package_archive_for_app(
        &resolver,
        archive_path.to_string_lossy().to_string(),
    )
    .await
    .expect("install package");

    let err = component_package_commands::render_installed_component_source_for_app(
        &resolver,
        &state,
        "bike.bottle-holder-kit".to_string(),
        "0.1.0".to_string(),
        "frame-rail".to_string(),
        Default::default(),
    )
    .await
    .expect_err("missing runtime target should fail");

    assert_eq!(
        err.message,
        "Installed component 'bike.bottle-holder-kit@0.1.0:frame-rail' port 'dovetail_rail' targetId 'missing-topology-target' was not found in rendered runtime topology."
    );

    fs::remove_dir_all(temp_root).ok();
}

#[tokio::test]
async fn installed_assembly_resolution_expands_instance_sources_and_exact_targets() {
    let mut package = sample_package();
    package.components[0].ports[0].target_ids = vec!["rail:edge:1".to_string()];
    package.components[1].ports[0].target_ids = vec!["cage:face:2".to_string()];

    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-assembly-resolve-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };
    let project_dir = temp_root.join("project");
    write_sample_component_sources(&project_dir);
    write_component_package_manifest(&project_dir, &package).expect("write manifest");
    let archive_path = temp_root.join("bike-kit.ecky");
    write_component_package_archive(&project_dir, &archive_path).expect("write archive");

    component_package_commands::install_component_package_archive_for_app(
        &resolver,
        archive_path.to_string_lossy().to_string(),
    )
    .await
    .expect("install package");

    let assembly = component_package_commands::resolve_installed_component_assembly_for_app(
        &resolver,
        "bike.bottle-holder-kit".to_string(),
        "0.1.0".to_string(),
        "bottle-holder".to_string(),
    )
    .await
    .expect("resolve installed assembly");

    assert_eq!(assembly.package_id, "bike.bottle-holder-kit");
    assert_eq!(assembly.assembly.assembly_id, "bottle-holder");
    assert_eq!(assembly.components.len(), 2);
    assert_eq!(assembly.mate_types.len(), 1);
    assert_eq!(assembly.port_types.len(), 2);
    assert_eq!(assembly.assembly.mates.len(), 1);
    assert_eq!(assembly.assembly.operations.len(), 1);
    assert_eq!(assembly.mate_results.len(), 1);
    assert!(assembly.mate_results[0].solved);
    assert_eq!(assembly.mate_results[0].warning, None);

    let rail = assembly
        .components
        .iter()
        .find(|component| component.instance_id == "rail")
        .expect("rail instance");
    let cage = assembly
        .components
        .iter()
        .find(|component| component.instance_id == "cage")
        .expect("cage instance");

    assert_eq!(rail.component_id, "frame-rail");
    assert_eq!(
        rail.installed_source.component.ports[0].target_ids,
        vec!["rail:edge:1".to_string()]
    );
    assert_eq!(rail.placement_frame, Some(PortFrame::identity()));
    assert_eq!(cage.component_id, "bottle-cage");
    assert_eq!(
        cage.installed_source.component.ports[0].target_ids,
        vec!["cage:face:2".to_string()]
    );
    assert_eq!(cage.placement_frame, Some(PortFrame::identity()));

    fs::remove_dir_all(temp_root).ok();
}

#[tokio::test]
async fn render_installed_assembly_returns_instance_runtimes_with_truthful_pending_flags() {
    let mut package = sample_package();
    for component in &mut package.components {
        component.source_language = Some(SourceLanguage::EckyIrV0);
        component.geometry_backend = Some(GeometryBackend::Freecad);
        component.macro_dialect = Some(MacroDialect::EckyIrV0);
    }
    package.components[1].ports[0].frame = None;

    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-assembly-render-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };
    let state = test_state(&temp_root);
    if !ecky_cad_lib::services::render::is_freecad_available(&state) {
        fs::remove_dir_all(temp_root).ok();
        return;
    }

    let project_dir = temp_root.join("project");
    write_sample_component_sources(&project_dir);
    write_component_package_manifest(&project_dir, &package).expect("write manifest");
    let archive_path = temp_root.join("bike-kit.ecky");
    write_component_package_archive(&project_dir, &archive_path).expect("write archive");

    component_package_commands::install_component_package_archive_for_app(
        &resolver,
        archive_path.to_string_lossy().to_string(),
    )
    .await
    .expect("install package");

    let assembly = component_package_commands::render_installed_component_assembly_for_app(
        &resolver,
        &state,
        "bike.bottle-holder-kit".to_string(),
        "0.1.0".to_string(),
        "bottle-holder".to_string(),
        Default::default(),
    )
    .await
    .expect("render installed assembly");

    assert_eq!(assembly.package_id, "bike.bottle-holder-kit");
    assert_eq!(assembly.assembly.assembly_id, "bottle-holder");
    assert_eq!(assembly.components.len(), 2);
    assert!(!assembly.mates_solved);
    assert!(!assembly.operations_applied);
    assert_eq!(assembly.operation_results.len(), 1);
    assert!(!assembly.operation_results[0].applied);
    assert_eq!(assembly.operation_results[0].operation_id, "fuse-holder");
    assert!(assembly.operation_results[0]
        .warning
        .as_deref()
        .unwrap_or_default()
        .contains("waiting on solved mates"));
    assert_eq!(assembly.mate_results.len(), 1);
    assert!(!assembly.mate_results[0].solved);
    assert_eq!(assembly.mate_results[0].mate_id, "rail-into-cage");
    assert!(assembly.mate_results[0]
        .warning
        .as_deref()
        .unwrap_or_default()
        .contains("missing frame"));
    assert!(assembly
        .warnings
        .iter()
        .any(|warning| warning.contains("missing frame")));
    assert!(assembly
        .warnings
        .iter()
        .any(|warning| warning.contains("operations are not applied yet")));
    assert!(assembly.components.iter().all(|component| component
        .runtime
        .artifact_bundle
        .source_language
        == SourceLanguage::EckyIrV0));
    assert!(assembly.components.iter().all(|component| component
        .runtime
        .artifact_bundle
        .geometry_backend
        == GeometryBackend::Freecad));
    assert!(assembly.components.iter().all(|component| !component
        .runtime
        .artifact_bundle
        .viewer_assets
        .is_empty()));

    fs::remove_dir_all(temp_root).ok();
}

#[tokio::test]
async fn render_installed_assembly_solves_linear_insert_port_frames_into_component_placements() {
    let mut package = sample_package();
    for component in &mut package.components {
        component.source_language = Some(SourceLanguage::EckyIrV0);
        component.geometry_backend = Some(GeometryBackend::Freecad);
        component.macro_dialect = Some(MacroDialect::EckyIrV0);
    }
    package.components[0].ports[0].frame = Some(PortFrame {
        origin: [10.0, 0.0, 5.0],
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
        z_axis: [0.0, 0.0, 1.0],
    });
    package.components[1].ports[0].frame = Some(PortFrame {
        origin: [-10.0, 0.0, 5.0],
        x_axis: [-1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
        z_axis: [0.0, 0.0, -1.0],
    });

    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-assembly-solve-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };
    let state = test_state(&temp_root);
    if !ecky_cad_lib::services::render::is_freecad_available(&state) {
        fs::remove_dir_all(temp_root).ok();
        return;
    }

    let project_dir = temp_root.join("project");
    write_sample_component_sources(&project_dir);
    write_component_package_manifest(&project_dir, &package).expect("write manifest");
    let archive_path = temp_root.join("bike-kit.ecky");
    write_component_package_archive(&project_dir, &archive_path).expect("write archive");

    component_package_commands::install_component_package_archive_for_app(
        &resolver,
        archive_path.to_string_lossy().to_string(),
    )
    .await
    .expect("install package");

    let assembly = component_package_commands::render_installed_component_assembly_for_app(
        &resolver,
        &state,
        "bike.bottle-holder-kit".to_string(),
        "0.1.0".to_string(),
        "bottle-holder".to_string(),
        Default::default(),
    )
    .await
    .expect("render installed assembly");

    assert!(assembly.mates_solved);
    assert!(!assembly.operations_applied);
    assert_eq!(assembly.operation_results.len(), 1);
    assert!(!assembly.operation_results[0].applied);
    assert_eq!(assembly.operation_results[0].operation_id, "fuse-holder");
    assert!(assembly.operation_results[0]
        .warning
        .as_deref()
        .unwrap_or_default()
        .contains("SeparateParts"));
    assert_eq!(assembly.mate_results.len(), 1);
    assert!(assembly.mate_results[0].solved);
    assert_eq!(assembly.mate_results[0].mate_id, "rail-into-cage");
    assert_eq!(assembly.mate_results[0].required_clearance, None);
    assert_eq!(assembly.mate_results[0].available_clearance, None);
    assert_eq!(assembly.mate_results[0].warning, None);
    assert!(assembly
        .warnings
        .iter()
        .all(|warning| !warning.contains("mates are not solved yet")));

    let rail = assembly
        .components
        .iter()
        .find(|component| component.instance_id == "rail")
        .expect("rail runtime");
    let cage = assembly
        .components
        .iter()
        .find(|component| component.instance_id == "cage")
        .expect("cage runtime");

    assert_eq!(rail.placement_frame, Some(PortFrame::identity()));
    assert_eq!(
        cage.placement_frame,
        Some(PortFrame {
            origin: [0.0, 0.0, 10.0],
            x_axis: [-1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
            z_axis: [0.0, 0.0, -1.0],
        })
    );

    fs::remove_dir_all(temp_root).ok();
}

#[tokio::test]
async fn render_installed_assembly_reports_clearance_rule_failure_in_mate_results() {
    let mut package = sample_package();
    for component in &mut package.components {
        component.source_language = Some(SourceLanguage::EckyIrV0);
        component.geometry_backend = Some(GeometryBackend::Freecad);
        component.macro_dialect = Some(MacroDialect::EckyIrV0);
    }
    package.components[0].ports[0].frame = Some(PortFrame {
        origin: [10.0, 0.0, 5.0],
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
        z_axis: [0.0, 0.0, 1.0],
    });
    package.components[1].ports[0].frame = Some(PortFrame {
        origin: [-10.0, 0.0, 5.0],
        x_axis: [-1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
        z_axis: [0.0, 0.0, -1.0],
    });
    package.components[0].ports[0].params.insert(
        "clearance".to_string(),
        ComponentInterfaceValue::Number(0.6),
    );
    package.components[1].ports[0].params.insert(
        "clearance".to_string(),
        ComponentInterfaceValue::Number(0.4),
    );
    package.assemblies[0].mates[0].params.insert(
        "clearance".to_string(),
        ComponentInterfaceValue::Number(0.5),
    );

    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-assembly-clearance-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };
    let state = test_state(&temp_root);
    if !ecky_cad_lib::services::render::is_freecad_available(&state) {
        fs::remove_dir_all(temp_root).ok();
        return;
    }

    let project_dir = temp_root.join("project");
    write_sample_component_sources(&project_dir);
    write_component_package_manifest(&project_dir, &package).expect("write manifest");
    let archive_path = temp_root.join("bike-kit.ecky");
    write_component_package_archive(&project_dir, &archive_path).expect("write archive");

    component_package_commands::install_component_package_archive_for_app(
        &resolver,
        archive_path.to_string_lossy().to_string(),
    )
    .await
    .expect("install package");

    let assembly = component_package_commands::render_installed_component_assembly_for_app(
        &resolver,
        &state,
        "bike.bottle-holder-kit".to_string(),
        "0.1.0".to_string(),
        "bottle-holder".to_string(),
        Default::default(),
    )
    .await
    .expect("render installed assembly");

    assert!(!assembly.mates_solved);
    assert_eq!(assembly.mate_results.len(), 1);
    assert!(!assembly.mate_results[0].solved);
    assert_eq!(assembly.mate_results[0].required_clearance, Some(0.5));
    assert_eq!(assembly.mate_results[0].available_clearance, Some(0.4));
    assert!(assembly.mate_results[0]
        .warning
        .as_deref()
        .unwrap_or_default()
        .contains("clearance"));
    assert!(assembly
        .warnings
        .iter()
        .any(|warning| warning.contains("clearance")));

    fs::remove_dir_all(temp_root).ok();
}

#[tokio::test]
async fn render_installed_assembly_merges_component_initial_params_per_instance() {
    let mut package = sample_package();
    for component in &mut package.components {
        component.source_language = Some(SourceLanguage::EckyIrV0);
        component.geometry_backend = Some(GeometryBackend::Build123d);
        component.macro_dialect = Some(MacroDialect::EckyIrV0);
    }

    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-render-assembly-default-params-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };
    let state = test_state(&temp_root);
    let capability = ecky_cad_lib::runtime_capabilities::probe_build123d_runtime(&resolver);
    if !capability.available {
        fs::remove_dir_all(temp_root).ok();
        return;
    }

    let project_dir = temp_root.join("project");
    write_sample_component_sources(&project_dir);
    write_component_package_manifest(&project_dir, &package).expect("write manifest");
    let archive_path = temp_root.join("bike-kit.ecky");
    write_component_package_archive(&project_dir, &archive_path).expect("write archive");

    component_package_commands::install_component_package_archive_for_app(
        &resolver,
        archive_path.to_string_lossy().to_string(),
    )
    .await
    .expect("install package");

    let assembly = component_package_commands::render_installed_component_assembly_for_app(
        &resolver,
        &state,
        "bike.bottle-holder-kit".to_string(),
        "0.1.0".to_string(),
        "bottle-holder".to_string(),
        [(
            "rail".to_string(),
            [(
                "mount_spacing".to_string(),
                ecky_cad_lib::models::ParamValue::Number(72.0),
            )]
            .into_iter()
            .collect(),
        )]
        .into_iter()
        .collect(),
    )
    .await
    .expect("render installed assembly");

    let rail = assembly
        .components
        .iter()
        .find(|component| component.instance_id == "rail")
        .expect("rail instance");
    let cage = assembly
        .components
        .iter()
        .find(|component| component.instance_id == "cage")
        .expect("cage instance");
    assert_eq!(
        rail.parameters.get("mount_spacing"),
        Some(&ecky_cad_lib::models::ParamValue::Number(72.0))
    );
    assert_eq!(
        cage.parameters.get("mount_spacing"),
        Some(&ecky_cad_lib::models::ParamValue::Number(64.0))
    );

    fs::remove_dir_all(temp_root).ok();
}

#[tokio::test]
async fn render_installed_assembly_builds_joined_output_runtime_when_output_mode_requests_it() {
    let mut package = sample_package();
    for component in &mut package.components {
        component.source_language = Some(SourceLanguage::EckyIrV0);
        component.geometry_backend = Some(GeometryBackend::Freecad);
        component.macro_dialect = Some(MacroDialect::EckyIrV0);
    }
    package.assemblies[0].operations.clear();
    package.assemblies[0].output.mode = AssemblyOutputMode::JoinedAssembly;
    package.components[0].ports[0].frame = Some(PortFrame {
        origin: [10.0, 0.0, 5.0],
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
        z_axis: [0.0, 0.0, 1.0],
    });
    package.components[1].ports[0].frame = Some(PortFrame {
        origin: [-10.0, 0.0, 5.0],
        x_axis: [-1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
        z_axis: [0.0, 0.0, -1.0],
    });

    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-assembly-joined-runtime-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };
    let state = test_state(&temp_root);
    if !ecky_cad_lib::services::render::is_freecad_available(&state) {
        fs::remove_dir_all(temp_root).ok();
        return;
    }

    let project_dir = temp_root.join("project");
    write_sample_component_sources(&project_dir);
    write_component_package_manifest(&project_dir, &package).expect("write manifest");
    let archive_path = temp_root.join("bike-kit.ecky");
    write_component_package_archive(&project_dir, &archive_path).expect("write archive");

    component_package_commands::install_component_package_archive_for_app(
        &resolver,
        archive_path.to_string_lossy().to_string(),
    )
    .await
    .expect("install package");

    let assembly = component_package_commands::render_installed_component_assembly_for_app(
        &resolver,
        &state,
        "bike.bottle-holder-kit".to_string(),
        "0.1.0".to_string(),
        "bottle-holder".to_string(),
        Default::default(),
    )
    .await
    .expect("render installed joined assembly");

    assert!(assembly.mates_solved);
    assert!(assembly.operations_applied);
    assert!(assembly.operation_results.is_empty());
    let output_runtime = assembly
        .output_runtime
        .as_ref()
        .expect("joined output runtime");
    assert_eq!(
        output_runtime.artifact_bundle.source_kind,
        ModelSourceKind::ImportedStep
    );
    assert!(std::path::Path::new(&output_runtime.artifact_bundle.preview_stl_path).is_file());
    assert!(std::path::Path::new(&output_runtime.artifact_bundle.fcstd_path).is_file());
    assert_eq!(output_runtime.artifact_bundle.export_artifacts.len(), 1);
    assert_eq!(
        output_runtime.artifact_bundle.export_artifacts[0].format,
        "step"
    );
    assert!(output_runtime.model_manifest.parts.len() >= 2);

    fs::remove_dir_all(temp_root).ok();
}

#[tokio::test]
async fn render_installed_assembly_builds_joined_output_runtime_with_partial_fuse_group() {
    let mut package = sample_package();
    for component in &mut package.components {
        component.source_language = Some(SourceLanguage::EckyIrV0);
        component.geometry_backend = Some(GeometryBackend::Freecad);
        component.macro_dialect = Some(MacroDialect::EckyIrV0);
    }
    package.components.push(sample_component(
        "spacer",
        "free_slot",
        "mechanical.dovetail.slot.v1",
        vec!["mechanical.dovetail.rail.v1".to_string()],
    ));
    package.components[0].ports[0].frame = Some(PortFrame {
        origin: [10.0, 0.0, 5.0],
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
        z_axis: [0.0, 0.0, 1.0],
    });
    package.components[1].ports[0].frame = Some(PortFrame {
        origin: [-10.0, 0.0, 5.0],
        x_axis: [-1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
        z_axis: [0.0, 0.0, -1.0],
    });
    package.components[2].ports[0].frame = Some(PortFrame::identity());
    package.assemblies[0].output.mode = AssemblyOutputMode::JoinedAssembly;
    package.assemblies[0].components.push(AssemblyComponentRef {
        instance_id: "spacer".to_string(),
        component_id: "spacer".to_string(),
    });
    package.assemblies[0].operations = vec![AssemblyOperation {
        operation_id: "fuse-holder".to_string(),
        kind: OperationKind::Fuse,
        target_instance_ids: vec!["rail".to_string(), "cage".to_string()],
        port_refs: Vec::new(),
        params: Default::default(),
    }];

    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-assembly-partial-fuse-runtime-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };
    let state = test_state(&temp_root);
    if !ecky_cad_lib::services::render::is_freecad_available(&state) {
        fs::remove_dir_all(temp_root).ok();
        return;
    }

    let project_dir = temp_root.join("project");
    write_sample_component_sources(&project_dir);
    let spacer_dir = project_dir.join("components").join("spacer");
    fs::create_dir_all(&spacer_dir).expect("spacer dir");
    fs::write(
        spacer_dir.join("source.ecky"),
        "(model (part body (box 20 20 10)))",
    )
    .expect("spacer source");
    write_component_package_manifest(&project_dir, &package).expect("write manifest");
    let archive_path = temp_root.join("bike-kit.ecky");
    write_component_package_archive(&project_dir, &archive_path).expect("write archive");

    component_package_commands::install_component_package_archive_for_app(
        &resolver,
        archive_path.to_string_lossy().to_string(),
    )
    .await
    .expect("install package");

    let assembly = component_package_commands::render_installed_component_assembly_for_app(
        &resolver,
        &state,
        "bike.bottle-holder-kit".to_string(),
        "0.1.0".to_string(),
        "bottle-holder".to_string(),
        Default::default(),
    )
    .await
    .expect("render installed joined partial-fuse assembly");

    assert!(assembly.mates_solved);
    assert!(assembly.operations_applied);
    assert_eq!(assembly.operation_results.len(), 1);
    assert!(assembly.operation_results[0].applied);
    assert_eq!(assembly.operation_results[0].operation_id, "fuse-holder");
    assert_eq!(
        assembly.operation_results[0].group_id.as_deref(),
        Some("fuse-group-1")
    );
    assert_eq!(assembly.operation_results[0].warning, None);
    assert_eq!(
        assembly.operation_results[0]
            .fusion_zone_ids_by_instance
            .get("rail")
            .map(String::as_str),
        Some("rear_spine_patch")
    );
    assert_eq!(
        assembly.operation_results[0]
            .fusion_zone_ids_by_instance
            .get("cage")
            .map(String::as_str),
        Some("rear_spine_patch")
    );
    let output_runtime = assembly
        .output_runtime
        .as_ref()
        .expect("joined partial-fuse output runtime");
    assert_eq!(output_runtime.model_manifest.parts.len(), 2);

    fs::remove_dir_all(temp_root).ok();
}

#[tokio::test]
async fn render_installed_assembly_reports_missing_fuse_zone_for_operation() {
    let mut package = sample_package();
    for component in &mut package.components {
        component.source_language = Some(SourceLanguage::EckyIrV0);
        component.geometry_backend = Some(GeometryBackend::Freecad);
        component.macro_dialect = Some(MacroDialect::EckyIrV0);
    }
    package.assemblies[0].output.mode = AssemblyOutputMode::JoinedAssembly;
    package.components[1].fusion_zones[0].allowed_ops = vec![OperationKind::Blend];
    package.components[0].ports[0].frame = Some(PortFrame {
        origin: [10.0, 0.0, 5.0],
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
        z_axis: [0.0, 0.0, 1.0],
    });
    package.components[1].ports[0].frame = Some(PortFrame {
        origin: [-10.0, 0.0, 5.0],
        x_axis: [-1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
        z_axis: [0.0, 0.0, -1.0],
    });

    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-assembly-fuse-zone-fail-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };
    let state = test_state(&temp_root);
    if !ecky_cad_lib::services::render::is_freecad_available(&state) {
        fs::remove_dir_all(temp_root).ok();
        return;
    }

    let project_dir = temp_root.join("project");
    write_sample_component_sources(&project_dir);
    write_component_package_manifest(&project_dir, &package).expect("write manifest");
    let archive_path = temp_root.join("bike-kit.ecky");
    write_component_package_archive(&project_dir, &archive_path).expect("write archive");

    component_package_commands::install_component_package_archive_for_app(
        &resolver,
        archive_path.to_string_lossy().to_string(),
    )
    .await
    .expect("install package");

    let assembly = component_package_commands::render_installed_component_assembly_for_app(
        &resolver,
        &state,
        "bike.bottle-holder-kit".to_string(),
        "0.1.0".to_string(),
        "bottle-holder".to_string(),
        Default::default(),
    )
    .await
    .expect("render installed fuse-zone-fail assembly");

    assert!(assembly.mates_solved);
    assert!(!assembly.operations_applied);
    assert!(assembly.output_runtime.is_none());
    assert_eq!(assembly.operation_results.len(), 1);
    assert!(!assembly.operation_results[0].applied);
    assert_eq!(assembly.operation_results[0].operation_id, "fuse-holder");
    assert!(assembly.operation_results[0]
        .warning
        .as_deref()
        .unwrap_or_default()
        .contains("missing Fuse-capable fusion zone"));
    assert!(assembly.operation_results[0]
        .warning
        .as_deref()
        .unwrap_or_default()
        .contains("cage"));

    fs::remove_dir_all(temp_root).ok();
}

#[tokio::test]
async fn render_installed_assembly_builds_joined_output_runtime_with_cut_group() {
    let mut package = sample_package();
    for component in &mut package.components {
        component.source_language = Some(SourceLanguage::EckyIrV0);
        component.geometry_backend = Some(GeometryBackend::Freecad);
        component.macro_dialect = Some(MacroDialect::EckyIrV0);
        component.fusion_zones[0].allowed_ops = vec![OperationKind::Cut];
    }
    package.assemblies[0].output.mode = AssemblyOutputMode::JoinedAssembly;
    package.assemblies[0].mates.clear();
    package.assemblies[0].operations = vec![AssemblyOperation {
        operation_id: "cut-slot".to_string(),
        kind: OperationKind::Cut,
        target_instance_ids: vec!["rail".to_string(), "cage".to_string()],
        port_refs: Vec::new(),
        params: Default::default(),
    }];

    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-assembly-cut-runtime-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };
    let state = test_state(&temp_root);
    if !ecky_cad_lib::services::render::is_freecad_available(&state) {
        fs::remove_dir_all(temp_root).ok();
        return;
    }

    let project_dir = temp_root.join("project");
    write_sample_component_sources(&project_dir);
    fs::write(
        project_dir
            .join("components")
            .join("bottle-cage")
            .join("source.ecky"),
        "(model (part body (box 10 10 10)))",
    )
    .expect("smaller cutter source");
    write_component_package_manifest(&project_dir, &package).expect("write manifest");
    let archive_path = temp_root.join("bike-kit.ecky");
    write_component_package_archive(&project_dir, &archive_path).expect("write archive");

    component_package_commands::install_component_package_archive_for_app(
        &resolver,
        archive_path.to_string_lossy().to_string(),
    )
    .await
    .expect("install package");

    let assembly = component_package_commands::render_installed_component_assembly_for_app(
        &resolver,
        &state,
        "bike.bottle-holder-kit".to_string(),
        "0.1.0".to_string(),
        "bottle-holder".to_string(),
        Default::default(),
    )
    .await
    .expect("render installed joined cut assembly");

    assert!(assembly.mates_solved);
    assert!(assembly.operations_applied);
    assert_eq!(assembly.operation_results.len(), 1);
    assert!(assembly.operation_results[0].applied);
    assert_eq!(assembly.operation_results[0].operation_id, "cut-slot");
    assert_eq!(
        assembly.operation_results[0].group_id.as_deref(),
        Some("cut-group-1")
    );
    assert_eq!(assembly.operation_results[0].warning, None);
    assert_eq!(
        assembly.operation_results[0]
            .fusion_zone_ids_by_instance
            .get("rail")
            .map(String::as_str),
        Some("rear_spine_patch")
    );
    assert_eq!(
        assembly.operation_results[0]
            .fusion_zone_ids_by_instance
            .get("cage")
            .map(String::as_str),
        Some("rear_spine_patch")
    );
    let output_runtime = assembly
        .output_runtime
        .as_ref()
        .expect("joined cut output runtime");
    assert_eq!(output_runtime.model_manifest.parts.len(), 1);
    assert!(
        !std::path::Path::new(&output_runtime.artifact_bundle.preview_stl_path)
            .as_os_str()
            .is_empty()
    );

    fs::remove_dir_all(temp_root).ok();
}

#[tokio::test]
async fn render_installed_assembly_reports_missing_cut_zone_for_operation() {
    let mut package = sample_package();
    for component in &mut package.components {
        component.source_language = Some(SourceLanguage::EckyIrV0);
        component.geometry_backend = Some(GeometryBackend::Freecad);
        component.macro_dialect = Some(MacroDialect::EckyIrV0);
        component.fusion_zones[0].allowed_ops = vec![OperationKind::Cut];
    }
    package.components[1].fusion_zones[0].allowed_ops = vec![OperationKind::Fuse];
    package.assemblies[0].output.mode = AssemblyOutputMode::JoinedAssembly;
    package.assemblies[0].mates.clear();
    package.assemblies[0].operations = vec![AssemblyOperation {
        operation_id: "cut-slot".to_string(),
        kind: OperationKind::Cut,
        target_instance_ids: vec!["rail".to_string(), "cage".to_string()],
        port_refs: Vec::new(),
        params: Default::default(),
    }];

    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-assembly-cut-zone-fail-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };
    let state = test_state(&temp_root);
    if !ecky_cad_lib::services::render::is_freecad_available(&state) {
        fs::remove_dir_all(temp_root).ok();
        return;
    }

    let project_dir = temp_root.join("project");
    write_sample_component_sources(&project_dir);
    write_component_package_manifest(&project_dir, &package).expect("write manifest");
    let archive_path = temp_root.join("bike-kit.ecky");
    write_component_package_archive(&project_dir, &archive_path).expect("write archive");

    component_package_commands::install_component_package_archive_for_app(
        &resolver,
        archive_path.to_string_lossy().to_string(),
    )
    .await
    .expect("install package");

    let assembly = component_package_commands::render_installed_component_assembly_for_app(
        &resolver,
        &state,
        "bike.bottle-holder-kit".to_string(),
        "0.1.0".to_string(),
        "bottle-holder".to_string(),
        Default::default(),
    )
    .await
    .expect("render installed cut-zone-fail assembly");

    assert!(assembly.mates_solved);
    assert!(!assembly.operations_applied);
    assert!(assembly.output_runtime.is_none());
    assert_eq!(assembly.operation_results.len(), 1);
    assert!(!assembly.operation_results[0].applied);
    assert_eq!(assembly.operation_results[0].operation_id, "cut-slot");
    assert!(assembly.operation_results[0]
        .warning
        .as_deref()
        .unwrap_or_default()
        .contains("missing Cut-capable fusion zone"));
    assert!(assembly.operation_results[0]
        .warning
        .as_deref()
        .unwrap_or_default()
        .contains("cage"));

    fs::remove_dir_all(temp_root).ok();
}

#[tokio::test]
async fn render_installed_assembly_builds_fused_output_runtime_for_pure_fuse_mode() {
    let mut package = sample_package();
    for component in &mut package.components {
        component.source_language = Some(SourceLanguage::EckyIrV0);
        component.geometry_backend = Some(GeometryBackend::Freecad);
        component.macro_dialect = Some(MacroDialect::EckyIrV0);
    }
    package.assemblies[0].output.mode = AssemblyOutputMode::FusedSolid;
    package.components[0].ports[0].frame = Some(PortFrame {
        origin: [10.0, 0.0, 5.0],
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
        z_axis: [0.0, 0.0, 1.0],
    });
    package.components[1].ports[0].frame = Some(PortFrame {
        origin: [-10.0, 0.0, 5.0],
        x_axis: [-1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
        z_axis: [0.0, 0.0, -1.0],
    });

    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-assembly-fused-runtime-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };
    let state = test_state(&temp_root);
    if !ecky_cad_lib::services::render::is_freecad_available(&state) {
        fs::remove_dir_all(temp_root).ok();
        return;
    }

    let project_dir = temp_root.join("project");
    write_sample_component_sources(&project_dir);
    write_component_package_manifest(&project_dir, &package).expect("write manifest");
    let archive_path = temp_root.join("bike-kit.ecky");
    write_component_package_archive(&project_dir, &archive_path).expect("write archive");

    component_package_commands::install_component_package_archive_for_app(
        &resolver,
        archive_path.to_string_lossy().to_string(),
    )
    .await
    .expect("install package");

    let assembly = component_package_commands::render_installed_component_assembly_for_app(
        &resolver,
        &state,
        "bike.bottle-holder-kit".to_string(),
        "0.1.0".to_string(),
        "bottle-holder".to_string(),
        Default::default(),
    )
    .await
    .expect("render installed fused assembly");

    assert!(assembly.mates_solved);
    assert!(assembly.operations_applied);
    assert_eq!(assembly.operation_results.len(), 1);
    assert!(assembly.operation_results[0].applied);
    assert_eq!(assembly.operation_results[0].operation_id, "fuse-holder");
    assert_eq!(
        assembly.operation_results[0].group_id.as_deref(),
        Some("fuse-group-1")
    );
    assert_eq!(assembly.operation_results[0].warning, None);
    assert_eq!(
        assembly.operation_results[0]
            .fusion_zone_ids_by_instance
            .get("rail")
            .map(String::as_str),
        Some("rear_spine_patch")
    );
    assert_eq!(
        assembly.operation_results[0]
            .fusion_zone_ids_by_instance
            .get("cage")
            .map(String::as_str),
        Some("rear_spine_patch")
    );
    let output_runtime = assembly
        .output_runtime
        .as_ref()
        .expect("fused output runtime");
    assert_eq!(
        output_runtime.artifact_bundle.source_kind,
        ModelSourceKind::ImportedStep
    );
    assert!(std::path::Path::new(&output_runtime.artifact_bundle.preview_stl_path).is_file());
    assert!(std::path::Path::new(&output_runtime.artifact_bundle.fcstd_path).is_file());
    assert_eq!(output_runtime.artifact_bundle.export_artifacts.len(), 1);
    assert_eq!(
        output_runtime.artifact_bundle.export_artifacts[0].format,
        "step"
    );
    assert_eq!(output_runtime.model_manifest.parts.len(), 1);

    fs::remove_dir_all(temp_root).ok();
}

#[tokio::test]
async fn export_installed_assembly_3mf_writes_placed_transform_items() {
    let mut package = sample_package();
    for component in &mut package.components {
        component.source_language = Some(SourceLanguage::EckyIrV0);
        component.geometry_backend = Some(GeometryBackend::Freecad);
        component.macro_dialect = Some(MacroDialect::EckyIrV0);
    }
    package.components[0].ports[0].frame = Some(PortFrame {
        origin: [10.0, 0.0, 5.0],
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
        z_axis: [0.0, 0.0, 1.0],
    });
    package.components[1].ports[0].frame = Some(PortFrame {
        origin: [-10.0, 0.0, 5.0],
        x_axis: [-1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
        z_axis: [0.0, 0.0, -1.0],
    });

    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-assembly-export-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };
    let state = test_state(&temp_root);
    if !ecky_cad_lib::services::render::is_freecad_available(&state) {
        fs::remove_dir_all(temp_root).ok();
        return;
    }

    let project_dir = temp_root.join("project");
    write_sample_component_sources(&project_dir);
    write_component_package_manifest(&project_dir, &package).expect("write manifest");
    let archive_path = temp_root.join("bike-kit.ecky");
    write_component_package_archive(&project_dir, &archive_path).expect("write archive");

    component_package_commands::install_component_package_archive_for_app(
        &resolver,
        archive_path.to_string_lossy().to_string(),
    )
    .await
    .expect("install package");

    let output_path = temp_root.join("bottle-holder.3mf");
    component_package_commands::export_installed_component_assembly_3mf_for_app(
        &resolver,
        &state,
        "bike.bottle-holder-kit".to_string(),
        "0.1.0".to_string(),
        "bottle-holder".to_string(),
        Default::default(),
        output_path.to_string_lossy().to_string(),
        None,
    )
    .await
    .expect("export installed assembly 3mf");

    let file = fs::File::open(&output_path).expect("3mf file");
    let mut archive = ZipArchive::new(file).expect("3mf archive");
    let mut model_xml = String::new();
    archive
        .by_name("3D/3dmodel.model")
        .expect("3mf model xml")
        .read_to_string(&mut model_xml)
        .expect("read 3mf model");

    assert!(model_xml.contains("<item objectid=\"1\" transform=\"1 0 0 0 0 1 0 0 0 0 1 0\"/>"));
    assert!(model_xml.contains("<item objectid=\"2\" transform=\"-1 0 0 0 0 1 0 0 0 0 -1 10\"/>"));

    fs::remove_dir_all(temp_root).ok();
}

#[tokio::test]
async fn export_installed_assembly_3mf_rejects_unsolved_mates() {
    let mut package = sample_package();
    for component in &mut package.components {
        component.source_language = Some(SourceLanguage::EckyIrV0);
        component.geometry_backend = Some(GeometryBackend::Freecad);
        component.macro_dialect = Some(MacroDialect::EckyIrV0);
    }
    package.components[1].ports[0].frame = None;

    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-assembly-export-fail-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };
    let state = test_state(&temp_root);
    if !ecky_cad_lib::services::render::is_freecad_available(&state) {
        fs::remove_dir_all(temp_root).ok();
        return;
    }

    let project_dir = temp_root.join("project");
    write_sample_component_sources(&project_dir);
    write_component_package_manifest(&project_dir, &package).expect("write manifest");
    let archive_path = temp_root.join("bike-kit.ecky");
    write_component_package_archive(&project_dir, &archive_path).expect("write archive");

    component_package_commands::install_component_package_archive_for_app(
        &resolver,
        archive_path.to_string_lossy().to_string(),
    )
    .await
    .expect("install package");

    let output_path = temp_root.join("bottle-holder.3mf");
    let err = component_package_commands::export_installed_component_assembly_3mf_for_app(
        &resolver,
        &state,
        "bike.bottle-holder-kit".to_string(),
        "0.1.0".to_string(),
        "bottle-holder".to_string(),
        Default::default(),
        output_path.to_string_lossy().to_string(),
        None,
    )
    .await
    .expect_err("unsolved assembly export should fail");

    assert!(err.message.contains("cannot export placed 3MF"));
    assert!(err.message.contains("missing frame"));

    fs::remove_dir_all(temp_root).ok();
}

#[tokio::test]
async fn export_installed_assembly_multipart_stl_zip_bakes_placement_into_part_meshes() {
    let mut package = sample_package();
    for component in &mut package.components {
        component.source_language = Some(SourceLanguage::EckyIrV0);
        component.geometry_backend = Some(GeometryBackend::Freecad);
        component.macro_dialect = Some(MacroDialect::EckyIrV0);
    }
    package.components[0].ports[0].frame = Some(PortFrame {
        origin: [10.0, 0.0, 5.0],
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
        z_axis: [0.0, 0.0, 1.0],
    });
    let cage_frame = PortFrame {
        origin: [-10.0, 0.0, 5.0],
        x_axis: [-1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
        z_axis: [0.0, 0.0, -1.0],
    };
    package.components[1].ports[0].frame = Some(cage_frame.clone());

    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-assembly-stl-zip-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };
    let state = test_state(&temp_root);
    if !ecky_cad_lib::services::render::is_freecad_available(&state) {
        fs::remove_dir_all(temp_root).ok();
        return;
    }

    let project_dir = temp_root.join("project");
    write_sample_component_sources(&project_dir);
    write_component_package_manifest(&project_dir, &package).expect("write manifest");
    let archive_path = temp_root.join("bike-kit.ecky");
    write_component_package_archive(&project_dir, &archive_path).expect("write archive");

    component_package_commands::install_component_package_archive_for_app(
        &resolver,
        archive_path.to_string_lossy().to_string(),
    )
    .await
    .expect("install package");

    let cage_runtime = component_package_commands::render_installed_component_source_for_app(
        &resolver,
        &state,
        "bike.bottle-holder-kit".to_string(),
        "0.1.0".to_string(),
        "bottle-cage".to_string(),
        Default::default(),
    )
    .await
    .expect("render installed cage component");
    let cage_asset_path = cage_runtime
        .artifact_bundle
        .viewer_assets
        .first()
        .expect("cage viewer asset")
        .path
        .clone();
    let local_triangles =
        read_binary_stl_triangles_from_reader(&mut fs::File::open(cage_asset_path).unwrap());

    let target_path = temp_root.join("placed-assembly.zip");
    component_package_commands::export_installed_component_assembly_multipart_stl_zip_for_app(
        &resolver,
        &state,
        "bike.bottle-holder-kit".to_string(),
        "0.1.0".to_string(),
        "bottle-holder".to_string(),
        Default::default(),
        target_path.to_string_lossy().to_string(),
        None,
    )
    .await
    .expect("export placed multipart stl zip");

    let zip_file = fs::File::open(&target_path).expect("placed stl zip");
    let mut archive = ZipArchive::new(zip_file).expect("zip archive");
    let cage_entry_name = (0..archive.len())
        .map(|index| archive.by_index(index).unwrap().name().to_string())
        .find(|name| name.contains("cage"))
        .expect("cage zip entry");
    let exported_triangles = {
        let mut entry = archive.by_name(&cage_entry_name).expect("cage entry");
        read_binary_stl_triangles_from_reader(&mut entry)
    };

    let expected_triangles = transform_stl_triangles(
        &local_triangles,
        &PortFrame {
            origin: [0.0, 0.0, 10.0],
            x_axis: [-1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
            z_axis: [0.0, 0.0, -1.0],
        },
    );
    assert_stl_triangles_approx_eq(&exported_triangles, &expected_triangles);

    fs::remove_dir_all(temp_root).ok();
}

#[tokio::test]
async fn export_installed_assembly_multipart_stl_zip_rejects_unsolved_mates() {
    let mut package = sample_package();
    for component in &mut package.components {
        component.source_language = Some(SourceLanguage::EckyIrV0);
        component.geometry_backend = Some(GeometryBackend::Freecad);
        component.macro_dialect = Some(MacroDialect::EckyIrV0);
    }
    package.components[1].ports[0].frame = None;

    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-assembly-stl-zip-fail-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };
    let state = test_state(&temp_root);
    if !ecky_cad_lib::services::render::is_freecad_available(&state) {
        fs::remove_dir_all(temp_root).ok();
        return;
    }

    let project_dir = temp_root.join("project");
    write_sample_component_sources(&project_dir);
    write_component_package_manifest(&project_dir, &package).expect("write manifest");
    let archive_path = temp_root.join("bike-kit.ecky");
    write_component_package_archive(&project_dir, &archive_path).expect("write archive");

    component_package_commands::install_component_package_archive_for_app(
        &resolver,
        archive_path.to_string_lossy().to_string(),
    )
    .await
    .expect("install package");

    let err =
        component_package_commands::export_installed_component_assembly_multipart_stl_zip_for_app(
            &resolver,
            &state,
            "bike.bottle-holder-kit".to_string(),
            "0.1.0".to_string(),
            "bottle-holder".to_string(),
            Default::default(),
            temp_root
                .join("placed-assembly.zip")
                .to_string_lossy()
                .to_string(),
            None,
        )
        .await
        .expect_err("unsolved mates should block placed stl zip export");

    assert!(err.message.contains("cannot export placed STL zip"));
    assert!(err.message.contains("missing frame"));

    fs::remove_dir_all(temp_root).ok();
}

#[test]
fn package_archive_rejects_missing_component_source_ref_files() {
    let package = sample_package();
    let temp_root = std::env::temp_dir().join(format!(
        "ecky-package-missing-source-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let project_dir = temp_root.join("project");
    fs::create_dir_all(&project_dir).expect("project dir");
    write_component_package_manifest(&project_dir, &package).expect("write manifest");
    let archive_path = temp_root.join("bike-kit.ecky");

    let err = write_component_package_archive(&project_dir, &archive_path)
        .expect_err("archive should fail without package-local sourceRef files");

    assert_eq!(
        err.message,
        format!(
            "Component package component 'frame-rail' sourceRef 'components/frame-rail/source.ecky' was not found under project dir '{}'.",
            project_dir.display()
        )
    );

    fs::remove_dir_all(temp_root).ok();
}
