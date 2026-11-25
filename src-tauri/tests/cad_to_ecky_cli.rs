//! Acceptance tests for the `cad_to_ecky` dev CLI — OpenSpec cad-transpile-engine
//! task 3.3: argument parsing + `--dump-prompt` output is **deterministic** and
//! **network-free**.
//!
//! Scope note: these tests pin ONLY the `--dump-prompt` slice (task 3.2) and the
//! arg-parsing shape around it. They deliberately do not exercise config
//! resolution, the LLM HTTP call, or the repair loop — those belong to other
//! tasks and are out of scope here. Source adapter dispatch is covered here
//! because it changes which text reaches the CLI prompt boundary.
//!
//! The network-free proof is real, not asserted: `--dump-prompt` is run with no
//! API key, no base URL, no config dir, `HOME` aimed at a throwaway temp dir, and
//! `--config` pointed at a file that does not exist. If that path read the
//! config or opened a socket it would either error or hang; a clean exit 0 with
//! the assembled prompt on stdout is therefore proof it did neither.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use ecky_cad_lib::cad_transpile::TRANSLATE_PREAMBLE;

/// Provider env vars that could silently satisfy a hidden network call. Stripped
/// from every invocation so the "no API call" assertion cannot be fooled by a
/// developer token lingering in the shell environment.
const PROVIDER_ENV: &[&str] = &[
    "NVIDIA_API_KEY",
    "NIM_API_KEY",
    "NVIDIA_BASE_URL",
    "NIM_BASE_URL",
    "NVIDIA_MODEL",
    "NIM_MODEL",
];

/// Foreign source used as the transpile input. The sentinel comment cannot
/// appear in the fixed translate preamble, so finding it verbatim in the USER
/// section proves the source is carried byte-for-byte.
const SOURCE: &str = "\
// SENTINEL_SOURCE_MARKER_7
// foreign openscad: a box plus a hex prism hint
cube([10, 20, 30]);
cylinder(h = 5, r = 2, $fn = 6);
";

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
            std::env::temp_dir().join(format!("cad-to-ecky-{name}-{}-{nonce}", std::process::id()));
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

fn cad_to_ecky() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cad_to_ecky"))
}

fn output_text(stream: &[u8]) -> String {
    String::from_utf8_lossy(stream).into_owned()
}

/// Strip every var that could satisfy a hidden network/config lookup, point HOME
/// at a throwaway dir, and aim `--config` at a nonexistent file. A clean success
/// under these conditions is the network-free proof.
fn hermetic_dump_prompt(input: &Path, home: &Path) -> (String, String, bool) {
    let mut cmd = cad_to_ecky();
    cmd.arg(input)
        .arg("--dump-prompt")
        .arg("--config")
        .arg(home.join("does-not-exist-config.edn"))
        .env("HOME", home)
        .env_remove("ECKY_APP_CONFIG_DIR");
    for var in PROVIDER_ENV {
        cmd.env_remove(var);
    }
    let output = cmd.output().expect("run cad_to_ecky --dump-prompt");
    (
        output_text(&output.stdout),
        output_text(&output.stderr),
        output.status.success(),
    )
}

fn config_edn(base_url: &str, api_key: &str, model: &str) -> String {
    format!(
        "{{:schema :ecky/config :version 1 :engines [{{:id \"selected\" :name \"selected\" :provider \"nim\" :api-key \"{api_key}\" :model \"{model}\" :base-url \"{base_url}\"}}] :selected-engine-id \"selected\"}}"
    )
}

fn spawn_openai_stub() -> (String, std::thread::JoinHandle<String>) {
    spawn_openai_stub_response("200 OK", r#"{"choices":[{"message":{"content":"ok"}}]}"#)
}

fn spawn_openai_stub_response(
    status: &'static str,
    body: &'static str,
) -> (String, std::thread::JoinHandle<String>) {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind OpenAI stub");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let receiver = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept CLI request");
        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        loop {
            let count = stream.read(&mut buffer).expect("read CLI request");
            request.extend_from_slice(&buffer[..count]);
            let Some(headers_end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n")
            else {
                continue;
            };
            let headers = String::from_utf8_lossy(&request[..headers_end]);
            let length = headers
                .lines()
                .find_map(|line| line.strip_prefix("content-length: "))
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(0);
            if request.len() >= headers_end + 4 + length {
                break;
            }
        }
        stream
            .write_all(format!("HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).as_bytes())
            .expect("respond to CLI");
        String::from_utf8_lossy(&request).into_owned()
    });
    (url, receiver)
}

fn configured_command(input: &Path, config_dir: &Path) -> Command {
    let mut command = cad_to_ecky();
    command.arg(input).env("ECKY_APP_CONFIG_DIR", config_dir);
    for var in PROVIDER_ENV {
        command.env_remove(var);
    }
    command
}

#[test]
fn cli_reads_canonical_edn_and_leaves_legacy_json_and_locks_untouched() {
    let dir = TestDir::new("edn-wins");
    let input = dir.path().join("in.scad");
    write_file(&input, SOURCE);
    let (url, receiver) = spawn_openai_stub();
    let edn = config_edn(&url, "edn-key", "edn-model");
    let json = r#"{"selectedEngineId":"wrong","engines":[]}"#;
    write_file(&dir.path().join("config.edn"), &edn);
    write_file(&dir.path().join("config.json"), json);

    let output = configured_command(&input, dir.path())
        .output()
        .expect("run CLI");
    assert!(
        output.status.success(),
        "stderr: {}",
        output_text(&output.stderr)
    );
    let request = receiver.join().expect("stub thread");
    assert!(
        request.contains("authorization: Bearer edn-key"),
        "{request}"
    );
    assert!(request.contains("\"model\":\"edn-model\""), "{request}");
    assert_eq!(
        fs::read_to_string(dir.path().join("config.edn")).unwrap(),
        edn
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("config.json")).unwrap(),
        json
    );
    assert!(!dir.path().join("config.lock").exists());
}

#[test]
fn invalid_edn_is_not_rescued_by_legacy_json() {
    let dir = TestDir::new("bad-edn");
    let input = dir.path().join("in.scad");
    write_file(&input, SOURCE);
    let secret = "INVALID_EDN_SECRET_SENTINEL";
    write_file(
        &dir.path().join("config.edn"),
        &format!("not valid edn \"{secret}\""),
    );
    let json = r#"{"selectedEngineId":"wrong","engines":[]}"#;
    write_file(&dir.path().join("config.json"), json);

    let output = configured_command(&input, dir.path())
        .output()
        .expect("run CLI");
    assert!(!output.status.success());
    let stderr = output_text(&output.stderr);
    assert!(stderr.contains("config.edn: invalid-data"), "{stderr}");
    assert!(!stderr.contains(secret), "{stderr}");
    assert!(
        !stderr.contains(&dir.path().display().to_string()),
        "{stderr}"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("config.json")).unwrap(),
        json
    );
    assert!(!dir.path().join("config.lock").exists());
}

#[test]
fn explicit_json_config_is_rejected_without_mutation() {
    let dir = TestDir::new("reject-json");
    let input = dir.path().join("in.scad");
    let path_secret = "EXPLICIT_JSON_PATH_SECRET_SENTINEL";
    let json_path = dir.path().join(format!("{path_secret}.json"));
    write_file(&input, SOURCE);
    write_file(&json_path, "{ legacy: true }");

    let mut command = cad_to_ecky();
    command.arg(&input).arg("--config").arg(&json_path);
    let output = command.output().expect("run CLI");
    assert!(!output.status.success());
    let stderr = output_text(&output.stderr);
    assert!(stderr.contains("canonical EDN only"));
    assert!(!stderr.contains(path_secret), "{stderr}");
    assert!(
        !stderr.contains(&dir.path().display().to_string()),
        "{stderr}"
    );
    assert_eq!(fs::read_to_string(&json_path).unwrap(), "{ legacy: true }");
    assert!(!dir.path().join("config.lock").exists());
}

#[test]
fn cli_flags_then_nvidia_then_nim_override_selected_engine_per_field() {
    let dir = TestDir::new("env-precedence");
    let input = dir.path().join("in.scad");
    write_file(&input, SOURCE);
    let (url, receiver) = spawn_openai_stub();
    write_file(
        &dir.path().join("config.edn"),
        &config_edn("http://unused.invalid", "config-key", "config-model"),
    );

    let mut command = configured_command(&input, dir.path());
    command
        .env("NIM_BASE_URL", "http://also-unused.invalid")
        .env("NVIDIA_BASE_URL", &url)
        .env("NIM_API_KEY", "nim-key")
        .env("NVIDIA_API_KEY", "nvidia-key")
        .env("NIM_MODEL", "nim-model")
        .env("NVIDIA_MODEL", "nvidia-model")
        .arg("--model")
        .arg("flag-model");
    let output = command.output().expect("run CLI");
    assert!(
        output.status.success(),
        "stderr: {}",
        output_text(&output.stderr)
    );
    let request = receiver.join().expect("stub thread");
    assert!(
        request.contains("authorization: Bearer nvidia-key"),
        "{request}"
    );
    assert!(request.contains("\"model\":\"flag-model\""), "{request}");
}

#[test]
fn config_and_provider_failures_redact_paths_ids_and_response_bodies() {
    let dir = TestDir::new("redaction");
    let input = dir.path().join("in.scad");
    write_file(&input, SOURCE);

    let selected_id_secret = "SELECTED_ENGINE_ID_SECRET_SENTINEL";
    write_file(
        &dir.path().join("config.edn"),
        &format!(
            "{{:schema :ecky/config :version 1 :engines [] :selected-engine-id \"{selected_id_secret}\"}}"
        ),
    );
    let output = configured_command(&input, dir.path())
        .output()
        .expect("run CLI");
    let stderr = output_text(&output.stderr);
    assert!(!output.status.success());
    assert!(
        stderr.contains("config.edn: selected-engine-missing"),
        "{stderr}"
    );
    assert!(!stderr.contains(selected_id_secret), "{stderr}");
    assert!(
        !stderr.contains(&dir.path().display().to_string()),
        "{stderr}"
    );

    let provider_secret = "PROVIDER_BODY_SECRET_SENTINEL";
    let (url, receiver) = spawn_openai_stub_response("401 Unauthorized", provider_secret);
    write_file(
        &dir.path().join("config.edn"),
        &config_edn(&url, "safe-test-key", "safe-test-model"),
    );
    let output = configured_command(&input, dir.path())
        .output()
        .expect("run CLI");
    let _request = receiver.join().expect("stub thread");
    let stderr = output_text(&output.stderr);
    assert!(!output.status.success());
    assert!(
        stderr.contains("HTTP 401 Unauthorized: provider-request-failed"),
        "{stderr}"
    );
    assert!(!stderr.contains(provider_secret), "{stderr}");
}

#[test]
fn missing_default_config_uses_environment_but_missing_explicit_config_fails_safely() {
    let dir = TestDir::new("missing-config");
    let input = dir.path().join("in.scad");
    write_file(&input, SOURCE);
    let (url, receiver) = spawn_openai_stub();
    let mut default_command = configured_command(&input, dir.path());
    default_command
        .env("NVIDIA_BASE_URL", &url)
        .env("NVIDIA_API_KEY", "env-key")
        .env("NVIDIA_MODEL", "env-model");
    let output = default_command
        .output()
        .expect("run without default config");
    assert!(
        output.status.success(),
        "stderr: {}",
        output_text(&output.stderr)
    );
    let _request = receiver.join().expect("stub thread");
    assert!(!dir.path().join("config.edn").exists());
    assert!(!dir.path().join("config.lock").exists());

    let path_secret = "MISSING_EXPLICIT_PATH_SECRET_SENTINEL";
    let explicit = dir.path().join(format!("{path_secret}.edn"));
    let mut explicit_command = cad_to_ecky();
    explicit_command.arg(&input).arg("--config").arg(&explicit);
    let output = explicit_command
        .output()
        .expect("run with missing explicit config");
    let stderr = output_text(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("config.edn: read-failed"), "{stderr}");
    assert!(!stderr.contains(path_secret), "{stderr}");
    assert!(
        !stderr.contains(&dir.path().display().to_string()),
        "{stderr}"
    );
}

#[test]
fn dump_prompt_emits_assembled_system_and_user_with_no_api_call() {
    let dir = TestDir::new("dump");
    let input = dir.path().join("in.scad");
    write_file(&input, SOURCE);

    let (stdout, stderr, ok) = hermetic_dump_prompt(&input, dir.path());

    // Network-free: succeeds with no config, no key, no base URL, no HOME config,
    // and a nonexistent --config. Any config read or HTTP attempt would either
    // error or hang here.
    assert!(
        ok,
        "--dump-prompt must succeed with no config/key (network-free)\nstderr:\n{stderr}"
    );
    assert!(
        stderr.trim().is_empty(),
        "--dump-prompt must not touch config or the network: {stderr}"
    );

    // System section = the shared language reference for the default backend.
    assert!(
        stdout.contains("===== SYSTEM (EckyRust) ====="),
        "default backend header missing\n{stdout}"
    );
    assert!(
        stdout.contains("# Ecky authoring"),
        "system prompt is not the shared language reference\n{stdout}"
    );

    // User section = the fixed translate instruction, then the source verbatim.
    assert!(
        stdout.contains("===== USER ====="),
        "USER section header missing\n{stdout}"
    );
    assert!(
        stdout.contains("Translate the CAD source below"),
        "translate instruction missing from USER section\n{stdout}"
    );
    assert!(
        stdout.contains(SOURCE),
        "foreign source must be carried verbatim into the USER section\n{stdout}"
    );

    // The foreign source must be carried BARE in the USER section, never fenced
    // by the dumper. The shared language reference legitimately documents Ecky
    // with its OWN fenced examples, so only a fence wrapping the *source* is a
    // bug — pin that precisely rather than banning every fence in the reference.
    assert!(
        !stdout.contains("```\n// SENTINEL_SOURCE_MARKER_7"),
        "--dump-prompt must not wrap the foreign source in a code fence\n{stdout}"
    );
}

#[test]
fn dump_prompt_routes_case_insensitive_scad_through_the_openscad_adapter() {
    let dir = TestDir::new("invalid-openscad");
    let input = dir.path().join("invalid.SCAD");
    fs::write(&input, b"cube();\xff").expect("write invalid OpenSCAD bytes");

    let (_stdout, stderr, ok) = hermetic_dump_prompt(&input, dir.path());

    assert!(
        !ok,
        "invalid UTF-8 .scad must not bypass the OpenSCAD adapter"
    );
    assert!(
        stderr.contains("read OpenSCAD source"),
        ".scad adapter decode error must reach CLI stderr: {stderr}"
    );
}

#[test]
fn dump_prompt_passes_unknown_extension_raw_text_verbatim() {
    let dir = TestDir::new("unknown-raw");
    let input = dir.path().join("fixture.pseudo-cad");
    // CRLF, Unicode, and a trailing newline distinguish this from a normalized
    // adapter result. The extension is deliberately not one claimed by a CAD
    // adapter, so it must take the raw-text fallback.
    let source = "# RAW_FALLBACK_SENTINEL \u{03bc}\r\nprimitive box 10 20 30\r\n";
    write_file(&input, source);

    let (stdout, stderr, ok) = hermetic_dump_prompt(&input, dir.path());

    assert!(
        ok,
        "unknown valid UTF-8 source must dump successfully: {stderr}"
    );
    let (_system, user) = stdout
        .split_once("===== USER =====\n")
        .expect("dump must contain exactly one USER section header");
    assert_eq!(
        user,
        format!("{TRANSLATE_PREAMBLE}\n{source}\n"),
        "unknown extension must produce exactly the fixed preamble + raw source + println newline"
    );
}

#[test]
fn dump_prompt_preserves_unknown_extension_raw_decode_error() {
    let dir = TestDir::new("unknown-invalid-utf8");
    let input = dir.path().join("fixture.pseudo-cad");
    fs::write(&input, b"RAW_FALLBACK_SENTINEL\xff").expect("write invalid raw bytes");

    let (_stdout, stderr, ok) = hermetic_dump_prompt(&input, dir.path());

    assert!(!ok, "invalid UTF-8 raw source must fail");
    assert!(
        stderr.contains("read input"),
        "raw read/decode error must reach CLI stderr: {stderr}"
    );
    assert!(
        !stderr.contains("read OpenSCAD source"),
        "unknown extension must not be sent through the OpenSCAD adapter: {stderr}"
    );
}

#[test]
fn dump_prompt_output_is_deterministic_across_runs() {
    let dir = TestDir::new("det");
    let input = dir.path().join("in.scad");
    write_file(&input, SOURCE);

    let (first, _, ok_first) = hermetic_dump_prompt(&input, dir.path());
    assert!(ok_first, "first run failed");
    let (second, _, ok_second) = hermetic_dump_prompt(&input, dir.path());
    assert!(ok_second, "second run failed");

    assert_eq!(
        first, second,
        "--dump-prompt output must be byte-identical across runs (deterministic)"
    );
}

#[test]
fn dump_prompt_respects_backend_flag() {
    let dir = TestDir::new("backend");
    let input = dir.path().join("in.scad");
    write_file(&input, SOURCE);

    // The serde aliases accepted on the CLI map to these Debug labels in the
    // SYSTEM header. Default (no flag) is EckyRust — pinned by the dump test.
    for (flag, label) in [
        ("mesh", "EckyRust"),
        ("freecad", "Freecad"),
        ("build123d", "Build123d"),
    ] {
        let mut cmd = cad_to_ecky();
        cmd.arg(&input)
            .arg("--dump-prompt")
            .arg("--backend")
            .arg(flag)
            .env("HOME", dir.path())
            .env_remove("ECKY_APP_CONFIG_DIR");
        for var in PROVIDER_ENV {
            cmd.env_remove(var);
        }
        let output = cmd
            .output()
            .expect("run cad_to_ecky --dump-prompt --backend");
        let stdout = output_text(&output.stdout);
        let stderr = output_text(&output.stderr);
        assert!(
            output.status.success(),
            "--backend {flag} failed\nstderr:\n{stderr}"
        );
        let expected = format!("===== SYSTEM ({label}) =====");
        assert!(
            stdout.contains(&expected),
            "--backend {flag} should select {label}: header `{expected}` missing\n{stdout}"
        );
    }
}

#[test]
fn dump_prompt_rejects_invalid_arguments() {
    let dir = TestDir::new("badargs");
    let input = dir.path().join("in.scad");
    write_file(&input, SOURCE);

    // Unknown backend value must fail with a clear message, not silently default.
    let mut cmd = cad_to_ecky();
    cmd.arg(&input)
        .arg("--dump-prompt")
        .arg("--backend")
        .arg("not-a-backend")
        .env("HOME", dir.path());
    let output = cmd.output().expect("run with unknown backend");
    let stderr = output_text(&output.stderr);
    assert!(
        !output.status.success(),
        "unknown backend must exit non-zero\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("unknown backend"),
        "unknown backend should be explained on stderr: {stderr}"
    );

    // Missing input positional must fail with usage, not panic.
    let mut cmd = cad_to_ecky();
    cmd.arg("--dump-prompt").env("HOME", dir.path());
    let output = cmd.output().expect("run with no input");
    let stderr = output_text(&output.stderr);
    assert!(
        !output.status.success(),
        "missing input must exit non-zero\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("Usage"),
        "missing input should print usage on stderr: {stderr}"
    );
}
