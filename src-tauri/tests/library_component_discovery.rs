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

#[test]
fn given_model_local_components_when_captured_then_parts_include_transitive_definitions() {
    use ecky_cad_lib::component_package_runtime as library;
    let paths = TestPaths::new();
    let source = r#"
        (define wall 2)
        (model
          (params (number width 12))
          (define-component dryer-base ((number width 12)) (box width wall 3))
          (define-component dryer-body ((number width 12)) (dryer-base :width width))
          (part enclosure-body (dryer-body :width width)))
    "#;
    library::capture_defined_components(&paths, "dryer", "v1", source).unwrap();
    let part = library::search_extracted_components(&paths, "enclosure-body", 20)
        .unwrap()
        .pop()
        .expect("model-local component call is reusable");
    let record = library::read_component_by_id(
        &paths,
        part.component_id.as_deref().unwrap(),
        part.revision_digest.as_deref(),
    )
    .unwrap();
    assert!(record.source.contains("(define-component dryer-base"));
    assert!(record.source.contains("(define-component dryer-body"));
    ecky_cad_lib::ecky_scheme::compile_to_core_program(&format!(
        "{} (model (part copied (enclosure-body)))",
        record.source
    ))
    .expect("captured dependency closure compiles in another model");
}

#[test]
fn given_nested_for_union_bindings_when_captured_then_loop_indices_are_not_free_references() {
    use ecky_cad_lib::component_package_runtime as library;
    let paths = TestPaths::new();
    let source = "(model (params (number count 2)) (part bracket
        (for-union (i count) (for-union (j 2) (box (+ 1 i) (+ 1 j) 1)))))";
    library::capture_defined_components(&paths, "bracket", "v1", source).unwrap();
    let part = library::search_extracted_components(&paths, "bracket", 20)
        .unwrap()
        .pop()
        .expect("loop-based part indexed");
    let record = library::read_component_by_id(
        &paths,
        part.component_id.as_deref().unwrap(),
        part.revision_digest.as_deref(),
    )
    .unwrap();
    assert_eq!(part.param_keys, ["count"]);
    ecky_cad_lib::ecky_scheme::compile_to_core_program(&format!(
        "{} (model (part copied (bracket)))",
        record.source
    ))
    .unwrap();
}

#[test]
fn given_indexed_component_when_header_becomes_unreadable_then_search_uses_persisted_metadata() {
    use ecky_cad_lib::component_package_runtime as library;
    let paths = TestPaths::new();
    library::capture_defined_components(
        &paths,
        "indexed",
        "v1",
        "(model (part bracket (box 2 2 2)))",
    )
    .unwrap();
    let before = library::search_extracted_components(&paths, "bracket", 20).unwrap();
    let part = before.iter().find(|item| item.origin == "local").unwrap();
    let header = paths
        .app_data_dir()
        .join("component-library/local-captures")
        .join(part.component_id.as_deref().unwrap())
        .join("revisions")
        .join(
            part.revision_digest
                .as_deref()
                .unwrap()
                .strip_prefix("sha256:")
                .unwrap(),
        )
        .join("ecky-header.json");
    std::fs::write(header, "unavailable").unwrap();
    let after = library::search_extracted_components(&paths, "bracket", 20).unwrap();
    assert!(
        after
            .iter()
            .any(|item| item.component_id == part.component_id),
        "normal search must not reopen source headers"
    );
}

#[test]
fn given_independent_valid_and_blocked_definitions_when_captured_then_valid_source_stays_discoverable(
) {
    use ecky_cad_lib::component_package_runtime as library;
    let paths = TestPaths::new();
    let source = "(define-component broken () (box missing 2 1)) (define-component good () (box 2 2 1)) (model (part body (good)))";
    assert!(library::capture_defined_components(&paths, "partial", "v1", source).is_err());
    let results = library::search_extracted_components(&paths, "good", 20).unwrap();
    assert!(
        results.iter().any(|entry| entry.name == "good"),
        "one invalid definition must not hide an independent valid definition"
    );
}

#[test]
fn given_installed_source_import_when_captured_then_reusable_source_is_closed() {
    use ecky_cad_lib::component_package_runtime as library;
    let paths = TestPaths::new();
    let project = paths.0.join("package");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("clip.ecky"),
        "(define-component clip () (box 3 2 1))",
    )
    .unwrap();
    let package = serde_json::from_value(json!({
        "schemaVersion": 1, "packageId": "fixture.clips", "version": "1.0.0", "displayName": "Clips", "visibility": "source",
        "components": [{"componentId": "clip", "version": "1.0.0", "displayName": "Clip", "sourceRef": "clip.ecky"}]
    })).unwrap();
    library::write_component_package_manifest(&project, &package).unwrap();
    let archive = paths.0.join("clips.ecky-package");
    library::write_component_package_archive(&project, &archive).unwrap();
    library::install_component_package_to_store(&paths, &archive).unwrap();
    let source = "(import-component \"fixture.clips\" :version \"1.0.0\" :component \"clip\" :as mount) (model (part body (mount)))";
    library::capture_defined_components(&paths, "imported", "v1", source).unwrap();
    let entry = library::search_extracted_components(&paths, "body", 100)
        .unwrap()
        .into_iter()
        .find(|entry| entry.thread_id.as_deref() == Some("imported"))
        .unwrap();
    let record = library::read_component_by_id(
        &paths,
        entry.component_id.as_deref().unwrap(),
        entry.revision_digest.as_deref(),
    )
    .unwrap();
    assert!(!record.source.contains("import-component"));
    ecky_cad_lib::ecky_scheme::compile_to_core_program(&format!(
        "{} (model (part reuse (body)))",
        record.source
    ))
    .unwrap();
}

#[test]
fn given_successful_python_model_when_cataloged_then_native_search_does_not_misclassify_it() {
    use ecky_cad_lib::{catalog, component_package_runtime as library, db};
    let paths = TestPaths::new();
    std::fs::create_dir_all(&paths.0).unwrap();
    let conn = db::init_db(&paths.0.join("history.sqlite")).unwrap();
    db::create_or_update_thread(&conn, "legacy-model", "Legacy fixture", 1, None).unwrap();
    let source = "import FreeCAD\npart = Part.makeBox(1, 2, 3)";
    let message=serde_json::from_value(json!({"id":"legacy-version","role":"assistant","content":"","status":"success","timestamp":1,
        "output":{"title":"Legacy fixture","macroCode":source,"macroDialect":"legacy","engineKind":"freecad","sourceLanguage":"legacyPython","geometryBackend":"freecad"},
        "artifactBundle":{"modelId":"legacy-model","sourceKind":"generated","engineKind":"freecad","sourceLanguage":"legacyPython","geometryBackend":"freecad","contentHash":"sha256:legacy","fcstdPath":"","manifestPath":"manifest","modelStlPath":"model.stl"}
    })).unwrap();
    db::add_message(&conn, "legacy-model", &message).unwrap();
    library::capture_latest_successful_components(
        &paths,
        &conn,
        "legacy-model",
        "legacy-version",
        source,
    )
    .unwrap();
    let query = catalog::CatalogQuery {
        query: "Legacy fixture".into(),
        kind: Some("model".into()),
        backend: Some("freecad".into()),
        ..Default::default()
    };
    let found = catalog::search(&paths, &query).unwrap();
    assert_eq!(
        found.results.len(),
        1,
        "legacy models must retain their actual authoring context"
    );
    assert_eq!(found.results[0].document.source_language, "legacyPython");
    assert!(catalog::search(
        &paths,
        &catalog::CatalogQuery {
            backend: Some("mesh".into()),
            ..query
        }
    )
    .unwrap()
    .results
    .is_empty());
}

#[tokio::test]
async fn given_old_component_revision_when_catalog_detail_is_requested_then_contract_matches_that_source(
) {
    let paths = TestPaths::new();
    let capture = |message: &str, source: &str| {
        ecky_cad_lib::component_package_runtime::capture_defined_components(
            &paths,
            "changing-thread",
            message,
            source,
        )
        .unwrap()
    };
    capture(
        "first",
        "(define-component changing-clip ((number width 4)) (box width 2 1))",
    );
    let hit = ecky_cad_lib::catalog::search(
        &paths,
        &ecky_cad_lib::catalog::CatalogQuery {
            query: "changing-clip".into(),
            ..Default::default()
        },
    )
    .unwrap()
    .results
    .remove(0);
    let revision = hit.document.reference["revisionDigest"]
        .as_str()
        .unwrap()
        .to_string();
    capture(
        "second",
        "(define-component changing-clip ((number depth 8)) (box 2 depth 1))",
    );
    let conn = ecky_cad_lib::db::init_db(&paths.app_data_dir().join("history.sqlite")).unwrap();
    let config = serde_json::from_value(json!({"engines":[],"selectedEngineId":""})).unwrap();
    let state = ecky_cad_lib::models::AppState::new(config, None, conn);
    let result = ecky_cad_lib::catalog::detail(
        &paths,
        &state,
        ecky_cad_lib::catalog::CatalogDetailRequest {
            id: hit.document.id,
            revision_digest: Some(revision.clone()),
            offset: None,
            max_chars: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(result["document"]["params"], json!(["width"]));
    assert_eq!(result["document"]["reference"]["revisionDigest"], revision);
    assert!(result["sourceWindow"].as_str().unwrap().contains("width"));
    assert!(!result["sourceWindow"].as_str().unwrap().contains("depth"));
}

#[test]
fn given_units_in_coordinate_lists_when_captured_then_literals_are_not_free_operators() {
    let source="(model (part token (polyhedron :vertices ((0mm 0mm 0mm) (3mm 0mm 0mm) (0mm 2mm 0mm) (0mm 0mm 1mm)) :triangles ((0 2 1) (0 1 3) (1 2 3) (2 0 3)))))";
    let component = ecky_cad_lib::component_extract::extract_component(
        &ecky_cad_lib::component_extract::ComponentExtractRequest {
            source: source.into(),
            part_key: "token".into(),
            ..Default::default()
        },
    )
    .unwrap();
    ecky_cad_lib::ecky_scheme::compile_to_core_program(&format!(
        "{}\n(model (part reused (token)))",
        component.component_source
    ))
    .unwrap();
}

#[test]
fn given_keyword_option_blocks_when_captured_then_keywords_are_not_free_operators() {
    let source="(model (part wall (wall-pattern (:mode \"ribs\" :depth 0.3 :uFreq 4 :vFreq 5) (box 10 10 2))))";
    let component = ecky_cad_lib::component_extract::extract_component(
        &ecky_cad_lib::component_extract::ComponentExtractRequest {
            source: source.into(),
            part_key: "wall".into(),
            ..Default::default()
        },
    )
    .unwrap();
    ecky_cad_lib::ecky_scheme::compile_to_core_program(&format!(
        "{}\n(model (part reused (wall)))",
        component.component_source
    ))
    .unwrap();
}

#[test]
fn given_domain_binders_and_literals_when_collecting_references_then_only_outer_values_are_free() {
    use std::collections::BTreeSet;
    use steel_core::parser::parser::Parser;
    for (source,expected) in [
        ("(mirror y 0 missing-geometry)",vec!["missing-geometry"]),
        ("(extrude input-profile extrusion-height :foreground dark)",vec!["extrusion-height","input-profile"]),
        ("(sampled-radial-loft (theta z fz) :height height :z-steps 4 :theta-steps 12 :radius (+ base (* fz (sin theta)) z) :z-map (* fz height))",vec!["base","height"]),
    ] {
        let forms=Parser::parse_without_lowering(source).unwrap();
        let free=ecky_cad_lib::ecky_scheme::compiler::collect_free_variables(&forms[0],&BTreeSet::new());
        assert_eq!(free,expected.into_iter().map(str::to_string).collect(),"{source}");
    }
}
