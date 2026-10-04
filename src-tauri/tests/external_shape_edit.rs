use ecky_cad_lib::contracts::{
    CaptureSurfaceAnchor, Config, DesignParams, GeometryBackend, MessageStatus, SourceLanguage,
    UiSpec,
};
use ecky_cad_lib::external_shapes::discover_bound_external_shapes;
use ecky_cad_lib::models::{AppState, PathResolver};
use ecky_cad_lib::services::design::{add_manual_version, AddManualVersionRequest};
use ecky_cad_lib::services::external_shape_edit::{
    apply_external_shape_edit, ApplyExternalShapeEditInput, ExternalShapeEditIntent,
};
use ecky_cad_lib::surface_trim_cap::SurfaceTrimCapMode;
use ecky_cad_lib::surface_trim_external_shapes::SurfaceTrimPathMode;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

struct TestResolver {
    root: PathBuf,
}

impl PathResolver for TestResolver {
    fn app_config_dir(&self) -> PathBuf {
        self.root.clone()
    }

    fn app_data_dir(&self) -> PathBuf {
        self.root.clone()
    }

    fn resource_path(&self, _path: &str) -> Option<PathBuf> {
        None
    }
}

fn test_config() -> Config {
    Config {
        engines: Vec::new(),
        selected_engine_id: String::new(),
        freecad_cmd: String::new(),
        cad_text_font_path: String::new(),
        freecad_library_roots: Vec::new(),
        assets: Vec::new(),
        microwave: None,
        voice: Default::default(),
        mcp: Default::default(),
        fem_compute: Default::default(),
        has_seen_onboarding: true,
        connection_type: None,
        provider_models: Default::default(),
        jev_classifier: Default::default(),
        default_engine_kind: ecky_cad_lib::contracts::EngineKind::EckyIrV0,
        default_source_language: SourceLanguage::EckyIrV0,
        default_geometry_backend: GeometryBackend::EckyRust,
        max_generation_attempts: 3,
        max_verify_attempts: 0,
        projects_root: None,
    }
}

fn cube_vertices() -> [[f64; 3]; 8] {
    [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 1.0],
    ]
}

fn cube_triangles() -> [([usize; 3], [f64; 3]); 12] {
    [
        ([0, 2, 1], [0.0, 0.0, -1.0]),
        ([0, 3, 2], [0.0, 0.0, -1.0]),
        ([4, 5, 6], [0.0, 0.0, 1.0]),
        ([4, 6, 7], [0.0, 0.0, 1.0]),
        ([0, 1, 5], [0.0, -1.0, 0.0]),
        ([0, 5, 4], [0.0, -1.0, 0.0]),
        ([1, 2, 6], [1.0, 0.0, 0.0]),
        ([1, 6, 5], [1.0, 0.0, 0.0]),
        ([2, 3, 7], [0.0, 1.0, 0.0]),
        ([2, 7, 6], [0.0, 1.0, 0.0]),
        ([3, 0, 4], [-1.0, 0.0, 0.0]),
        ([3, 4, 7], [-1.0, 0.0, 0.0]),
    ]
}

fn write_cube_stl(path: &Path) {
    let vertices = cube_vertices();
    let mut stl = String::from("solid cube\n");
    for (triangle, normal) in cube_triangles() {
        writeln!(
            stl,
            "facet normal {} {} {}",
            normal[0], normal[1], normal[2]
        )
        .unwrap();
        stl.push_str("  outer loop\n");
        for vertex_index in triangle {
            let vertex = vertices[vertex_index];
            writeln!(stl, "    vertex {} {} {}", vertex[0], vertex[1], vertex[2]).unwrap();
        }
        stl.push_str("  endloop\nendfacet\n");
    }
    stl.push_str("endsolid cube\n");
    std::fs::write(path, stl).expect("write cube STL");
}

fn anchor(mesh_digest: &str, triangle_index: usize, barycentric: [f64; 3]) -> CaptureSurfaceAnchor {
    let vertices = cube_vertices();
    let (triangle, normal) = cube_triangles()[triangle_index];
    let source_position = triangle.iter().enumerate().fold(
        [0.0, 0.0, 0.0],
        |mut point, (weight_index, vertex_index)| {
            let vertex = vertices[*vertex_index];
            let weight = barycentric[weight_index];
            point[0] += vertex[0] * weight;
            point[1] += vertex[1] * weight;
            point[2] += vertex[2] * weight;
            point
        },
    );
    CaptureSurfaceAnchor {
        source_mesh_content_digest: mesh_digest.to_string(),
        triangle_index: triangle_index as u64,
        barycentric,
        source_position,
        source_normal: normal,
    }
}

struct Fixture {
    root: PathBuf,
    resolver: TestResolver,
    state: AppState,
    base_message_id: String,
    source: String,
    node_id: u64,
    source_digest: String,
    mesh_digest: String,
}

async fn fixture(extra_part: &str) -> Fixture {
    let root =
        std::env::temp_dir().join(format!("ecky-external-shape-edit-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("temp root");
    let stl_path = root.join("cube.stl");
    write_cube_stl(&stl_path);
    let source = format!(
        "(model (part scanned (import-stl {:?})) {})",
        stl_path.to_string_lossy(),
        extra_part
    );
    let discovered = discover_bound_external_shapes(&source, &root).expect("external source");
    let selected = discovered.first().expect("selected source");
    let conn = ecky_cad_lib::db::init_db(&root.join("history.sqlite")).expect("db");
    let state = AppState::new(test_config(), None, conn);
    let resolver = TestResolver { root: root.clone() };
    let base_message_id = add_manual_version(
        AddManualVersionRequest {
            thread_id: "thread-1".to_string(),
            title: "External cube".to_string(),
            version_name: "V1".to_string(),
            macro_code: source.clone(),
            source_language: Some(SourceLanguage::EckyIrV0),
            geometry_backend: Some(GeometryBackend::EckyRust),
            parameters: DesignParams::new(),
            ui_spec: UiSpec::default(),
            post_processing: None,
            artifact_bundle: None,
            model_manifest: None,
            response_text: None,
            agent_origin: None,
            status: None,
            error_message: None,
        },
        &state,
        &resolver,
    )
    .await
    .expect("base version");
    Fixture {
        root,
        resolver,
        state,
        base_message_id,
        source,
        node_id: selected.node_id,
        source_digest: selected.source_digest.clone(),
        mesh_digest: selected.content_digest.clone().expect("mesh digest"),
    }
}

fn crop_anchors(fixture: &Fixture) -> Vec<CaptureSurfaceAnchor> {
    vec![
        anchor(&fixture.mesh_digest, 2, [1.0, 0.0, 0.0]),
        anchor(&fixture.mesh_digest, 2, [0.0, 1.0, 0.0]),
        anchor(&fixture.mesh_digest, 2, [0.0, 0.0, 1.0]),
    ]
}

fn trim_loop(fixture: &Fixture) -> Vec<CaptureSurfaceAnchor> {
    vec![
        anchor(&fixture.mesh_digest, 4, [0.25, 0.25, 0.5]),
        anchor(&fixture.mesh_digest, 7, [0.5, 0.25, 0.25]),
        anchor(&fixture.mesh_digest, 6, [0.25, 0.25, 0.5]),
        anchor(&fixture.mesh_digest, 9, [0.5, 0.25, 0.25]),
        anchor(&fixture.mesh_digest, 8, [0.25, 0.25, 0.5]),
        anchor(&fixture.mesh_digest, 11, [0.5, 0.25, 0.25]),
        anchor(&fixture.mesh_digest, 10, [0.25, 0.25, 0.5]),
        anchor(&fixture.mesh_digest, 5, [0.5, 0.25, 0.25]),
    ]
}

fn input(fixture: &Fixture, edit: ExternalShapeEditIntent) -> ApplyExternalShapeEditInput {
    ApplyExternalShapeEditInput {
        thread_id: "thread-1".to_string(),
        base_message_id: Some(fixture.base_message_id.clone()),
        expected_source_digest: fixture.source_digest.clone(),
        edit,
    }
}

#[tokio::test]
async fn plane_crop_and_remove_append_versions_and_return_canonical_sources() {
    let fixture = fixture("").await;
    let applied = apply_external_shape_edit(
        input(
            &fixture,
            ExternalShapeEditIntent::ApplyPlaneCrop {
                node_id: fixture.node_id,
                expected_mesh_content_digest: fixture.mesh_digest.clone(),
                anchors: crop_anchors(&fixture),
                keep_positive: false,
                replace_crop_node_id: None,
            },
        ),
        &fixture.state,
        &fixture.resolver,
    )
    .await
    .expect("apply crop");
    assert_eq!(applied.version.status, MessageStatus::Success);
    assert!(applied.version.artifact_bundle.is_some());
    assert!(applied.version.model_manifest.is_some());
    assert_eq!(applied.external_sources[0].plane_crops.len(), 1);
    let crop_node_id = applied.external_sources[0].plane_crops[0].node_id;

    let removed = apply_external_shape_edit(
        ApplyExternalShapeEditInput {
            thread_id: "thread-1".to_string(),
            base_message_id: applied.version.message_id.clone(),
            expected_source_digest: applied.source_digest.clone(),
            edit: ExternalShapeEditIntent::RemovePlaneCrop {
                node_id: applied.external_sources[0].node_id,
                crop_node_id,
            },
        },
        &fixture.state,
        &fixture.resolver,
    )
    .await
    .expect("remove crop");
    assert_eq!(removed.version.status, MessageStatus::Success);
    assert!(removed.external_sources[0].plane_crops.is_empty());
    assert_eq!(removed.version.design_output.macro_code, fixture.source);
    std::fs::remove_dir_all(fixture.root).expect("cleanup");
}

#[tokio::test]
async fn surface_trim_and_remove_append_versions_and_return_canonical_sources() {
    let fixture = fixture("(part unrelated (box 2 2 2))").await;
    let applied = apply_external_shape_edit(
        input(
            &fixture,
            ExternalShapeEditIntent::ApplySurfaceTrim {
                schema_version: 1,
                node_id: fixture.node_id,
                expected_mesh_content_digest: fixture.mesh_digest.clone(),
                loop_anchors: trim_loop(&fixture),
                keep_seed: anchor(&fixture.mesh_digest, 2, [1.0 / 3.0; 3]),
                path_mode: SurfaceTrimPathMode::Shortest,
                cap_mode: SurfaceTrimCapMode::Flat,
                replace_trim_node_id: None,
            },
        ),
        &fixture.state,
        &fixture.resolver,
    )
    .await
    .expect("apply trim");
    assert_eq!(applied.version.status, MessageStatus::Success);
    assert_eq!(applied.external_sources[0].surface_trims.len(), 1);
    let trim_node_id = applied.external_sources[0].surface_trims[0].node_id;

    let removed = apply_external_shape_edit(
        ApplyExternalShapeEditInput {
            thread_id: "thread-1".to_string(),
            base_message_id: applied.version.message_id.clone(),
            expected_source_digest: applied.source_digest.clone(),
            edit: ExternalShapeEditIntent::RemoveSurfaceTrim {
                node_id: applied.external_sources[0].node_id,
                trim_node_id,
            },
        },
        &fixture.state,
        &fixture.resolver,
    )
    .await
    .expect("remove trim");
    assert_eq!(removed.version.status, MessageStatus::Success);
    assert!(removed.external_sources[0].surface_trims.is_empty());
    std::fs::remove_dir_all(fixture.root).expect("cleanup");
}

#[tokio::test]
async fn stale_source_digest_rejects_before_version_append() {
    let fixture = fixture("").await;
    let before = {
        let conn = fixture.state.db.lock().await;
        ecky_cad_lib::db::get_thread_latest_version(&conn, "thread-1")
            .expect("latest")
            .expect("base")
            .id
    };
    let mut request = input(
        &fixture,
        ExternalShapeEditIntent::ApplyPlaneCrop {
            node_id: fixture.node_id,
            expected_mesh_content_digest: fixture.mesh_digest.clone(),
            anchors: crop_anchors(&fixture),
            keep_positive: false,
            replace_crop_node_id: None,
        },
    );
    request.expected_source_digest = "sha256:stale".to_string();

    let error = apply_external_shape_edit(request, &fixture.state, &fixture.resolver)
        .await
        .expect_err("stale source");
    assert!(error.message.contains("changed before plane crop"));
    let after = {
        let conn = fixture.state.db.lock().await;
        ecky_cad_lib::db::get_thread_latest_version(&conn, "thread-1")
            .expect("latest")
            .expect("base")
            .id
    };
    assert_eq!(before, after);
    std::fs::remove_dir_all(fixture.root).expect("cleanup");
}

#[tokio::test]
async fn render_failure_returns_raw_error_and_one_failed_immutable_version() {
    let fixture = fixture("(part broken (sphere -1))").await;
    let result = apply_external_shape_edit(
        input(
            &fixture,
            ExternalShapeEditIntent::ApplyPlaneCrop {
                node_id: fixture.node_id,
                expected_mesh_content_digest: fixture.mesh_digest.clone(),
                anchors: crop_anchors(&fixture),
                keep_positive: false,
                replace_crop_node_id: None,
            },
        ),
        &fixture.state,
        &fixture.resolver,
    )
    .await
    .expect("domain failure result");
    assert_eq!(result.version.status, MessageStatus::Error);
    assert!(!result
        .version
        .error
        .as_ref()
        .expect("raw render error")
        .message
        .trim()
        .is_empty());
    let conn = fixture.state.db.lock().await;
    let head = ecky_cad_lib::db::get_thread_latest_version(&conn, "thread-1")
        .expect("latest")
        .expect("failed version");
    assert_eq!(head.status, MessageStatus::Error);
    assert_eq!(Some(head.id.as_str()), result.version.message_id.as_deref());
    drop(conn);
    std::fs::remove_dir_all(fixture.root).expect("cleanup");
}

#[test]
fn external_shape_edit_contract_serializes_camel_case_tagged_intent() {
    let value = serde_json::to_value(ApplyExternalShapeEditInput {
        thread_id: "thread-1".to_string(),
        base_message_id: Some("message-1".to_string()),
        expected_source_digest: "sha256:source".to_string(),
        edit: ExternalShapeEditIntent::RemovePlaneCrop {
            node_id: 4,
            crop_node_id: 7,
        },
    })
    .expect("serialize contract");
    assert_eq!(value["threadId"], "thread-1");
    assert_eq!(value["baseMessageId"], "message-1");
    assert_eq!(value["expectedSourceDigest"], "sha256:source");
    assert_eq!(value["edit"]["action"], "removePlaneCrop");
    assert_eq!(value["edit"]["cropNodeId"], 7);
    assert!(value.get("thread_id").is_none());
}
