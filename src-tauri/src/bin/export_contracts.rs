use std::fs;
use std::path::PathBuf;

use specta_typescript::{BigIntExportBehavior, Typescript};

fn main() {
    let builder = ecky_cad_lib::bindings::builder();
    let output_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src/lib/tauri/contracts.ts");
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).expect("Failed to create contract output directory");
    }
    builder
        .export(
            Typescript::default().bigint(BigIntExportBehavior::Number),
            &output_path,
        )
        .expect("Failed to export TypeScript contracts");

    let generated = fs::read_to_string(&output_path).expect("Failed to read generated contracts");
    let patched = generated.replace("window.emit(name, arg)", "(window as any).emit(name, arg)");
    if patched != generated {
        fs::write(&output_path, patched).expect("Failed to patch generated contracts");
    }
}
