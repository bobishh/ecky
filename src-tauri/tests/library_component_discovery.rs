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
