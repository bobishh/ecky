use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct WorkedExampleManifest {
    project: String,
    stages: Vec<WorkedExampleStage>,
}

#[derive(Debug, Deserialize)]
struct WorkedExampleStage {
    id: String,
    title: String,
    source: String,
    focus: Vec<String>,
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri must live below repository root")
        .to_path_buf()
}

#[test]
fn worked_book_examples_compile_in_declared_order() {
    let root = repository_root();
    let manifest_path = root.join("docs/books/ecky-ir/examples/toothbrush-holder/manifest.json");
    let manifest: WorkedExampleManifest = serde_json::from_str(
        &fs::read_to_string(&manifest_path).expect("read worked-example manifest"),
    )
    .expect("parse worked-example manifest");

    assert_eq!(manifest.project, "Perforated toothbrush holder");
    assert_eq!(
        manifest
            .stages
            .iter()
            .map(|stage| stage.id.as_str())
            .collect::<Vec<_>>(),
        ["shell", "drained-base", "single-cutter", "repeated-cutters"]
    );

    for stage in manifest.stages {
        assert!(!stage.title.trim().is_empty(), "{} needs a title", stage.id);
        assert!(
            !stage.focus.is_empty(),
            "{} needs a teaching focus",
            stage.id
        );
        let source_path = root.join(&stage.source);
        let source = fs::read_to_string(&source_path)
            .unwrap_or_else(|error| panic!("read {}: {error}", source_path.display()));
        ecky_cad_lib::ecky_scheme::compile_to_core_program(&source)
            .unwrap_or_else(|error| panic!("compile {}: {error}", source_path.display()));
    }
}

#[test]
fn all_worm_farm_downloads_compile_with_declared_checks() {
    let examples = repository_root().join("docs/books/ecky-ir/examples/worm-farm");
    let mut count = 0;
    for entry in fs::read_dir(examples).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|value| value.to_str()) != Some("ecky") {
            continue;
        }
        let source = fs::read_to_string(&path).unwrap();
        ecky_cad_lib::ecky_scheme::compile_to_core_program(&source)
            .unwrap_or_else(|error| panic!("compile {}: {error}", path.display()));
        count += 1;
    }
    assert_eq!(count, 9);
}
