use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(name: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before unix epoch")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("ecky-cli-{name}-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&path).expect("create temp test dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn write_file(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap_or_else(|err| {
        panic!("write {} failed: {err}", path.display());
    });
}

fn ecky_command() -> Command {
    let bin = std::env::var("CARGO_BIN_EXE_ecky")
        .expect("CARGO_BIN_EXE_ecky must be set by cargo during test runs");
    Command::new(bin)
}

fn output_text(stream: &[u8]) -> String {
    String::from_utf8_lossy(stream).into_owned()
}

#[test]
fn check_accepts_simple_model_source() {
    let dir = TestDir::new("check-valid");
    let input_path = dir.path().join("input.ecky");
    write_file(&input_path, "(model (part body (box 1 2 3)))");

    let output = ecky_command()
        .arg("check")
        .arg(&input_path)
        .output()
        .expect("run ecky check");
    let stdout = output_text(&output.stdout);
    let stderr = output_text(&output.stderr);

    assert!(
        output.status.success(),
        "check should succeed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.trim().is_empty(),
        "check success should keep stderr empty: {stderr}"
    );
}

#[test]
fn check_reports_compile_error_on_stderr_for_invalid_source() {
    let dir = TestDir::new("check-invalid");
    let input_path = dir.path().join("invalid.ecky");
    write_file(&input_path, "(model\n  (part body (box 1 2 3))\n$)");

    let output = ecky_command()
        .arg("check")
        .arg(&input_path)
        .output()
        .expect("run ecky check");
    let stderr = output_text(&output.stderr);

    assert!(
        !output.status.success(),
        "check should fail for invalid source\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("Expected a proper list for model form."),
        "stderr should surface compile error\nstderr:\n{stderr}"
    );
}

#[test]
fn lower_build123d_writes_requested_output_file() {
    let dir = TestDir::new("lower-build123d");
    let input_path = dir.path().join("input.ecky");
    let output_path = dir.path().join("lowered.py");
    write_file(&input_path, "(model (part body (box 1 2 3)))");

    let output = ecky_command()
        .arg("lower")
        .arg("--backend")
        .arg("build123d")
        .arg(&input_path)
        .arg("--out")
        .arg(&output_path)
        .output()
        .expect("run ecky lower");
    let stdout = output_text(&output.stdout);
    let stderr = output_text(&output.stderr);

    assert!(
        output.status.success(),
        "lower should succeed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        output_path.is_file(),
        "missing output file: {}",
        output_path.display()
    );

    let lowered = fs::read_to_string(&output_path).unwrap_or_else(|err| {
        panic!("read {} failed: {err}", output_path.display());
    });
    assert!(
        lowered.contains("from build123d import *"),
        "lowered build123d file missing expected import\n{lowered}"
    );
}
