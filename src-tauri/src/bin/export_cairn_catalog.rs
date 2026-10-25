//! Export only reviewed repository sources; never traverse a user's model history.
use ecky_cad_lib::{
    component_extract, component_package_runtime as packages, contracts::ComponentPackage,
    models::PathResolver,
};
use serde_json::json;
use std::{fs, path::PathBuf};

struct Paths(PathBuf);
impl PathResolver for Paths {
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
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or("Usage: export_cairn_catalog OUTPUT_DIR")?,
    );
    fs::create_dir_all(&output)?;
    let scratch = std::env::temp_dir().join(format!("cairn-export-{}", uuid::Uuid::new_v4()));
    let paths = Paths(scratch.clone());
    let result = export(&paths, &output);
    let _ = fs::remove_dir_all(scratch);
    result
}
fn export(paths: &Paths, output: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let stdlib = output.join("ecky.stdlib");
    fs::create_dir_all(&stdlib)?;
    let mut exports = Vec::new();
    for entry in packages::search_extracted_components(paths, "", usize::MAX)? {
        if entry.origin != "builtin" {
            continue;
        }
        let record = packages::read_extracted_component(paths, &entry.name)?;
        // Publication must not turn a malformed shipped definition into a permanent release.
        component_extract::extract_defined_components(&record.source, "repository", "stdlib")?;
        ecky_cad_lib::ecky_scheme::compile_to_core_program(&format!(
            "{}\n(model (part sample ({})))",
            record.source, entry.name
        ))?;
        let file = format!("{}.ecky", entry.name);
        fs::write(stdlib.join(&file), record.source)?;
        exports.push(json!({"componentId":entry.name,"version":"1.0.0","displayName":entry.one_liner,"sourceRef":file,"sourceLanguage":"ecky","geometryBackend":"mesh","params":entry.param_keys.iter().map(|key|json!({"key":key,"label":key,"kind":"number"})).collect::<Vec<_>>()}));
    }
    let manifest = json!({"schemaVersion":1,"packageId":"ecky.stdlib","version":"1.0.0","displayName":"Ecky standard components","visibility":"source","tags":["fastener","mounting","parametric"],"components":exports,"cairn":{"repository":"https://github.com/bobishh/ecky","dependencies":[]}});
    fs::write(
        stdlib.join("ecky-package.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    let release = packages::write_distributable_component_package_archive(
        &stdlib,
        &output.join("ecky.stdlib-1.0.0.ecky-package"),
    )?;
    println!(
        "{}@{}: {} exports",
        release.package_id,
        release.version,
        exports.len()
    );
    let stones = repository.join("packages/cairn.stones");
    let source = fs::read_to_string(stones.join("stone.ecky"))?;
    ecky_cad_lib::ecky_scheme::compile_to_core_program(&format!(
        "{source}\n(model (part sample (stone)))"
    ))?;
    packages::write_distributable_component_package_archive(
        &stones,
        &output.join("cairn.stones-0.1.0.ecky-package"),
    )?;
    let kit = output.join("ecky.starter");
    fs::create_dir_all(&kit)?;
    let package: ComponentPackage = serde_json::from_value(
        json!({"schemaVersion":1,"packageId":"ecky.starter","version":"0.1.0","displayName":"Ecky starter collection","visibility":"source","components":[],"tags":["collection","starter","stone","fastener"],"cairn":{"repository":"https://github.com/bobishh/ecky","dependencies":[{"packageId":"ecky.stdlib","version":"1.0.0"},{"packageId":"cairn.stones","version":"0.1.0"}]}}),
    )?;
    fs::write(
        kit.join("ecky-package.json"),
        serde_json::to_vec_pretty(&package)?,
    )?;
    packages::write_distributable_component_package_archive(
        &kit,
        &output.join("ecky.starter-0.1.0.ecky-package"),
    )?;
    println!("cairn.stones@0.1.0 and ecky.starter@0.1.0 exported");
    Ok(())
}
