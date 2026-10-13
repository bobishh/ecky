use ecky_render::scheme::SchemeSourceCompiler;
use ecky_render::SourceCompiler;
use std::path::{Path, PathBuf};

fn collect_sources(root: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(root).expect("read book source directory") {
        let path = entry.expect("book entry").path();
        if path.is_dir() {
            collect_sources(&path, files);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "ecky")
        {
            files.push(path);
        }
    }
}

#[test]
fn every_book_source_compiles_before_publication() {
    // Given the complete book source tree, when models are compiled, no broken
    // supplemental checkpoint may hide behind a successfully built HTML page.
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap();
    let mut files = Vec::new();
    collect_sources(&repository.join("docs/books/ecky-ir"), &mut files);
    files.sort();
    assert!(
        files.len() >= 70,
        "expected full book, not a reduced fixture"
    );
    let failures: Vec<_> = files
        .iter()
        .filter_map(|path| {
            let source = std::fs::read_to_string(path).expect("read model");
            SchemeSourceCompiler
                .compile(&source)
                .err()
                .map(|error| format!("{}: {}", path.display(), error.message))
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
