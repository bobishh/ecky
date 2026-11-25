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
    let patched = generated
        // specta-typescript leaves trailing whitespace before doc-comment
        // continuations on multi-line type exports; strip it so `git diff
        // --check` stays clean across regenerations.
        .lines()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
        .replace("window.emit(name, arg)", "(window as any).emit(name, arg)")
        // `componentImportOrigins` is a serde-defaulted compatibility field.
        // Specta rc does not mark this Vec optional, so preserve the legacy
        // frontend contract here until the generator handles it natively.
        .replace(
            "componentImportOrigins: ComponentImportOrigin[]",
            "componentImportOrigins?: ComponentImportOrigin[]",
        )
        .replace(
            "componentPlacementEvidence: ComponentPlacementEvidence[]",
            "componentPlacementEvidence?: ComponentPlacementEvidence[]",
        )
        .replace("productionPending: boolean", "productionPending?: boolean")
        .replace(
            "assemblyTree: AssemblyNode[]",
            "assemblyTree?: AssemblyNode[]",
        );
    if patched != generated {
        fs::write(&output_path, patched).expect("Failed to patch generated contracts");
    }
}
