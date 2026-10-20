use ecky_cad_lib::{models::PathResolver, services::library_panel};
use serde_json::json;
use std::path::PathBuf;

struct TestPaths(PathBuf);

impl TestPaths {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("ecky-library-discovery-{}", uuid::Uuid::new_v4())))
    }
}

impl Drop for TestPaths {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl PathResolver for TestPaths {
    fn app_config_dir(&self) -> PathBuf {
        self.0.join("config")
    }
    fn app_data_dir(&self) -> PathBuf {
        self.0.join("data")
    }
    fn resource_path(&self, _: &str) -> Option<PathBuf> {
        None
    }
}

#[test]
fn given_saved_component_without_packages_when_library_loads_then_local_and_builtin_headers_are_visible(
) {
    let paths = TestPaths::new();
    let component = paths.app_data_dir().join("component-library/bottle-cage");
    std::fs::create_dir_all(&component).unwrap();
    std::fs::write(
        component.join("ecky-header.json"),
        serde_json::to_vec(&json!({
            "name": "bottle-cage",
            "description": "Reusable cage",
            "params": [{"key": "diameter", "kind": "number", "default": 74}],
            "tags": ["bike"],
            "provenance": {"sourceDigest": "saved-source"},
            "interfaces": [],
            "ports": []
        }))
        .unwrap(),
    )
    .unwrap();
    // Listing must work from headers alone, without loading source bodies.
    let projection =
        serde_json::to_value(library_panel::load_component_packages(&paths).unwrap()).unwrap();
    assert_eq!(projection["packageHeaders"], json!([]));
    let components = projection["components"]
        .as_array()
        .expect("library projection includes available components");
    assert!(components.iter().any(|item| item["name"] == "hex-bolt"));
    let saved = components
        .iter()
        .find(|item| item["name"] == "bottle-cage")
        .unwrap();
    assert_eq!(saved["paramKeys"], json!(["diameter"]));
    assert_eq!(saved["oneLiner"], "Reusable cage");
    assert!(saved.get("source").is_none());
}

#[test]
fn given_unreadable_library_location_when_loading_then_storage_failure_is_reported() {
    let paths = TestPaths::new();
    std::fs::create_dir_all(paths.app_data_dir()).unwrap();
    std::fs::write(
        paths.app_data_dir().join("component-library"),
        "not a directory",
    )
    .unwrap();
    let error = library_panel::load_component_packages(&paths).unwrap_err();
    assert!(error.to_string().contains("component library directory"));
}

#[test]
fn given_rendered_parts_when_captured_then_search_and_pinned_reuse_preserve_revisions() {
    use ecky_cad_lib::{component_package_runtime as library, mcp::handlers};
    let paths = TestPaths::new();
    let source =
        "(define wall 2) (model (params (number width 12)) (part bracket (box width wall 3)))";
    library::capture_defined_components(&paths, "design-a", "v1", source).unwrap();
    let results = library::search_extracted_components(&paths, "bracket", 20).unwrap();
    let bracket = results
        .iter()
        .find(|item| item.name == "bracket")
        .expect("rendered part indexed");
    let id = bracket.component_id.as_deref().unwrap();
    let revision = bracket.revision_digest.as_deref().unwrap();
    assert!(library::search_extracted_components(&paths, "width", 20)
        .unwrap()
        .iter()
        .any(|item| item.component_id.as_deref() == Some(id)));
    library::capture_defined_components(
        &paths,
        "design-a",
        "v2",
        &source.replace("wall 2", "wall 4"),
    )
    .unwrap();
    let latest = library::read_component_by_id(&paths, id, None).unwrap();
    assert_ne!(latest.revision_digest.as_deref(), Some(revision));
    let request = serde_json::from_value(json!({
        "componentId": id, "revisionDigest": revision,
        "source": "(model (part base (box 1 1 1)))"
    }))
    .unwrap();
    let imported = handlers::handle_component_import(&paths, request).unwrap();
    assert!(imported.authored_source.contains("(define wall 2)"));
    assert!(!imported.authored_source.contains("(define wall 4)"));
    assert!(imported
        .authored_source
        .contains("(part bracket (bracket))"));
    let repeated = handlers::handle_component_import(
        &paths,
        serde_json::from_value(json!({
            "componentId": id, "revisionDigest": revision, "source": imported.authored_source,
        }))
        .unwrap(),
    )
    .unwrap();
    assert!(repeated
        .authored_source
        .contains("(part bracket-2 (bracket))"));
    assert_eq!(
        repeated
            .authored_source
            .matches("(define-component bracket")
            .count(),
        1
    );
    let conflict = handlers::handle_component_import(
        &paths,
        serde_json::from_value(json!({
            "componentId": id, "revisionDigest": revision,
            "source": "(define wall 99) (model (part base (box 1 1 1)))",
        }))
        .unwrap(),
    )
    .unwrap_err();
    assert!(conflict
        .to_string()
        .contains("already exists with different source"));
    ecky_cad_lib::ecky_scheme::compile_to_core_program(&imported.authored_source)
        .expect("reused part compiles");
    library::capture_defined_components(&paths, "design-b", "v3", source).unwrap();
    assert_eq!(
        library::search_extracted_components(&paths, "bracket", 20)
            .unwrap()
            .iter()
            .filter(|item| item.name == "bracket" && item.component_id.as_deref() != Some(id))
            .count(),
        1
    );
}

#[test]
fn given_one_unsupported_part_when_captured_then_valid_siblings_remain_reusable() {
    use ecky_cad_lib::component_package_runtime as library;
    let paths = TestPaths::new();
    let error = library::capture_defined_components(
        &paths,
        "design-a",
        "v1",
        "(model (part unsupported (missing-helper)) (part valid (box 2 2 2)))",
    )
    .unwrap_err();
    assert!(error.to_string().contains("unsupported"));
    assert!(library::search_extracted_components(&paths, "valid", 20)
        .unwrap()
        .iter()
        .any(|component| component.name == "valid"));
}

#[test]
fn given_derived_lexical_geometry_when_captured_then_reused_part_compiles() {
    use ecky_cad_lib::component_package_runtime as library;
    let paths = TestPaths::new();
    library::capture_defined_components(&paths, "derived", "v1",
        "(model (params (number width 12)) (let ((wall 2)) (let* ((inner (- width wall)) (half (/ inner 2))) (part half-panel (box half wall 1)))))"
    ).unwrap();
    let component = library::search_extracted_components(&paths, "half-panel", 20)
        .unwrap()
        .pop()
        .unwrap();
    let record = library::read_component_by_id(
        &paths,
        component.component_id.as_deref().unwrap(),
        component.revision_digest.as_deref(),
    )
    .unwrap();
    ecky_cad_lib::ecky_scheme::compile_to_core_program(&format!(
        "{} (model (part copied (half-panel)))",
        record.source
    ))
    .unwrap();
}
