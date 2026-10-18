use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

#[test]
fn dev_scripts_do_not_write_application_sqlite_directly() {
    let root = repo_root();
    let scripts = root.join("scripts");
    let mut files = Vec::new();
    collect_files(&scripts, &mut files);

    let allowlist = [scripts.join("guard_no_direct_db_write.sh")];
    let forbidden = [
        "history.sqlite",
        "sqlite3 ",
        "sqlite3\n",
        "sqlite3\t",
        "sqlite3.connect",
        "INSERT INTO",
        "UPDATE ",
        "DELETE FROM",
    ];

    let mut violations = Vec::new();
    for path in files {
        if allowlist.iter().any(|allowed| allowed == &path) {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        for pattern in forbidden {
            if text.contains(pattern) {
                violations.push(format!("{} contains {}", path.display(), pattern));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "direct DB write patterns must stay out of dev scripts:\n{}",
        violations.join("\n")
    );
}

#[test]
fn production_export_modules_do_not_reference_debug_overlay_geometry() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let export_modules = [
        manifest_dir.join("src/commands/render.rs"),
        manifest_dir.join("src/commands/component_package.rs"),
        manifest_dir.join("src/services/render.rs"),
    ];
    let forbidden = ["ShapeGraphDebugOverlay", "debug_overlay", "debug overlay"];

    let mut violations = Vec::new();
    for path in export_modules {
        let text = fs::read_to_string(&path).expect("read export module");
        for pattern in forbidden {
            if text.contains(pattern) {
                violations.push(format!("{} contains {}", path.display(), pattern));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "debug overlays are preview diagnostics and must not enter production export modules:\n{}",
        violations.join("\n")
    );
}

#[test]
fn rust_sources_do_not_reference_local_application_history_paths() {
    let root = repo_root();
    let source = root.join("src-tauri/src");
    let mut files = Vec::new();
    collect_files(&source, &mut files);

    let forbidden = ["Application Support/com.alcoholics-audacious.ecky-cad/history.sqlite"];
    let mut violations = Vec::new();
    for path in files {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        if forbidden.iter().any(|pattern| text.contains(pattern)) {
            violations.push(path.display().to_string());
        }
    }

    assert!(
        violations.is_empty(),
        "Rust source must not reference local application history paths:\n{}",
        violations.join("\n")
    );
}

#[test]
fn frontend_app_code_does_not_call_tauri_invoke_directly() {
    let root = repo_root();
    let source = root.join("src");
    let mut files = Vec::new();
    collect_files(&source, &mut files);

    let allowlist = [
        source.join("lib/tauri/contracts.ts"),
        source.join("lib/docs/downloadAsset.test.ts"),
    ];
    let mut violations = Vec::new();
    for path in files {
        if allowlist.iter().any(|allowed| allowed == &path) {
            continue;
        }
        let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
            continue;
        };
        if !matches!(extension, "ts" | "svelte") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        if text.contains("@tauri-apps/api/core")
            && (text.contains("import { invoke")
                || text.contains("import {invoke")
                || text.contains("invoke as"))
        {
            violations.push(path.display().to_string());
        }
    }

    assert!(
        violations.is_empty(),
        "frontend command calls must go through src/lib/tauri/contracts.ts and src/lib/tauri/client.ts:\n{}",
        violations.join("\n")
    );
}
