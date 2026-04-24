use std::fs;
use std::io::{Read, Write};

use base64::Engine as _;
use ecky_cad_lib::commands::component_package as component_package_commands;
use ecky_cad_lib::component_package_runtime::{
    extract_component_package_archive, install_component_package_archive,
    list_installed_component_package_headers, read_component_package_from_archive,
    read_component_package_header_from_archive, read_component_package_manifest,
    write_component_package_archive, write_component_package_manifest, COMPONENT_PACKAGE_FILE_NAME,
    COMPONENT_PACKAGE_HEADER_FILE_NAME,
};
use ecky_cad_lib::models::{
    component_package_header, validate_component_package, AssemblyComponentRef, AssemblyDefinition,
    AssemblyMate, AssemblyOperation, AssemblyOutput, AssemblyOutputMode, ComponentDefinition,
    ComponentFusionZone, ComponentKeepoutVolume, ComponentPackage, ComponentParam,
    ComponentParamKind, ComponentPort, KeepoutVolumeKind, MatePortTypePair, MateTypeDefinition,
    OperationKind, PackageVisibility, PathResolver, PortFrame, PortReference, PortTypeDefinition,
    SketchConstraint, SketchConstraintKind, SketchDefinition, SketchPrimitive, SketchPrimitiveKind,
    SketchView, COMPONENT_PACKAGE_SCHEMA_VERSION,
};

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
    let payload_dir = project_dir.join("components/frame-rail");
    fs::create_dir_all(&payload_dir).expect("payload dir");
    fs::write(payload_dir.join("source.ecky"), "(model)").expect("payload file");

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
    let payload_dir = project_dir.join("components/frame-rail");
    fs::create_dir_all(&payload_dir).expect("payload dir");
    fs::write(payload_dir.join("source.ecky"), "(model)").expect("payload file");
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
    assert_eq!(header.assemblies[0].mate_count, 1);
    assert_eq!(header.assemblies[0].operation_count, 1);
    assert!(header_json["components"][0].get("sourceRef").is_none());
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

    assert_eq!(installed.header.package_id, "bike.bottle-holder-kit");
    assert_eq!(headers.len(), 1);

    fs::remove_dir_all(temp_root).ok();
}
