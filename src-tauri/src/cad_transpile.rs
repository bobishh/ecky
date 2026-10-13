//! CAD transpile — a thin LLM translate over the shared Ecky language reference.
//!
//! Transpile is not an engine. It assembles the same self-contained Ecky system
//! prompt the agent already uses (`agent_prompt::agent_language_reference`) plus a
//! fixed translate instruction, sends the foreign CAD source as the user message
//! through the existing OpenAI-compatible client, and returns `.ecky`. The
//! compile + `verify` gate (elsewhere) decides trust; this module only builds the
//! request and (in the binary) performs the call.

use crate::agent_prompt::agent_language_reference;
use crate::contracts::{Config, GeometryBackend};
use crate::llm::{extract_openai_message_content, send_openai_request};
use serde_json::json;
use std::path::Path;

/// Default OpenAI-compatible endpoint when the selected engine omits a base URL.
pub const NVIDIA_NIM_BASE_URL: &str = "https://integrate.api.nvidia.com/v1";

/// Selected engine connection after documented environment overrides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranspileConnection {
    pub provider: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

/// Geometry evidence used by the internal source-parity tier.
#[derive(Debug, Clone, PartialEq)]
pub struct GeometryMeasurement {
    pub bbox: crate::contracts::ManifestBounds,
    pub volume: f64,
}

/// Relative error limits for source-versus-rendered geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeometryParityTolerance {
    pub bbox_relative: f64,
    pub volume_relative: f64,
}

impl Default for GeometryParityTolerance {
    fn default() -> Self {
        Self {
            bbox_relative: 0.02,
            volume_relative: 0.05,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum GeometryParityResult {
    NotRequested,
    Passed {
        bbox_relative_error: [f64; 3],
        volume_relative_error: f64,
    },
    Failed {
        diagnostic: String,
        bbox_relative_error: Option<[f64; 3]>,
        volume_relative_error: Option<f64>,
    },
}

/// Render identity plus optional geometry evidence. Rendering stays owned by the
/// established runtime; this type only carries its result into the gate.
#[derive(Debug, Clone, PartialEq)]
pub struct TranspileRender {
    pub model_id: String,
    pub measurement: Option<GeometryMeasurement>,
}

/// Existing compile/render and `verify_generated_model` stages, injected so the
/// shared gate has no CLI, Tauri, thread, or persistence dependency.
pub trait TranspileGateRuntime {
    fn compile_and_render(&mut self, source: &str) -> Result<TranspileRender, String>;
    fn verify_generated_model(
        &mut self,
        model_id: &str,
    ) -> Result<crate::contracts::StructuralVerificationResult, String>;
}

/// User-stated requirements retained across repair turns.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DialogueRequirements {
    entries: Vec<String>,
}

impl DialogueRequirements {
    pub fn record(&mut self, requirement: impl AsRef<str>) {
        let requirement = requirement.as_ref().trim();
        if requirement.is_empty() || self.entries.iter().any(|entry| entry == requirement) {
            return;
        }
        self.entries.push(requirement.to_string());
    }

    pub fn as_slice(&self) -> &[String] {
        &self.entries
    }

    pub fn repair_prompt(&self, diagnostic: &str) -> String {
        let mut prompt = format!(
            "The transpiled Ecky failed its authoritative compile/render/verify gate.\n\nDIAGNOSTIC:\n{diagnostic}\n\nFix the named cause and emit one complete `(model ...)` program only."
        );
        if !self.entries.is_empty() {
            prompt.push_str(
                "\n\nPreserve every dialogue requirement below. Apply each geometry change and add a matching `(verify ...)` clause so the requirement persists:\n",
            );
            for requirement in &self.entries {
                prompt.push_str("- ");
                prompt.push_str(requirement);
                prompt.push('\n');
            }
        }
        prompt
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranspileRepairRequest {
    pub failed_source: String,
    pub diagnostic: String,
    pub prompt: String,
    pub repair_number: u32,
}

/// Gate result contains an explicit commit signal. Callers must never commit
/// when `ready_to_commit` is false.
#[derive(Debug, Clone, PartialEq)]
pub struct TranspileGateOutcome {
    pub source: String,
    pub ready_to_commit: bool,
    pub attempts: u32,
    pub diagnostic: Option<String>,
    pub parity: GeometryParityResult,
}

/// Fixed translate instruction prepended to the foreign source in the user
/// message. It carries the *semantic* ask the deterministic transpiler could not
/// do (parametrize, loop-ify) plus the distinction between round primitives and
/// explicit polygonal geometry. Round primitives accept dimensions only.
pub const TRANSLATE_PREAMBLE: &str = "\
Translate the CAD source below into ONE parametric Ecky `(model ...)` program.

- Infer meaningful numeric parameters (sizes, counts, repeats) into a `(params ...)`
  block; do not copy dead numbers. Derive dependent dimensions as expressions.
- Express repeated features as loops (`repeat-union` / `for-union`), never as N
  hand-copied translated solids.
- Round primitives accept dimensions only: `(cylinder radius height)`,
  `(cone bottom-radius top-radius height)`, `(circle radius)`, and
  `(ring outer-radius inner-radius)`. For a polygonal prism (e.g. a hex bolt
  head), use `(extrude (regular-polygon SIDES circumradius) height)`.
- Add `(verify ...)` clauses for the invariants that must hold (at least a single
  watertight solid: `stl connected-component-count` = 1, `stl
  non-manifold-edge-count` = 0).
- Output ONLY Ecky source — no prose, no code fences.

CAD source:
";

/// Build the `(system, user)` message pair for a transpile request. `system` is
/// the shared, drift-free Ecky language reference for `backend`; `user` is the
/// fixed translate instruction followed by the source verbatim.
pub fn build_transpile_messages(source: &str, backend: GeometryBackend) -> (String, String) {
    let system = agent_language_reference(backend);
    let user = format!("{TRANSLATE_PREAMBLE}\n{source}");
    (system, user)
}

/// Resolve the selected app-config engine, then apply process-local overrides.
///
/// `NVIDIA_API_KEY`, `NVIDIA_BASE_URL`, and `NVIDIA_MODEL` take precedence over
/// `NIM_API_KEY`, `NIM_BASE_URL`, and `NIM_MODEL`; either set overrides the
/// matching selected-engine value. Empty base URLs fall back to NVIDIA NIM.
/// The caller supplies `env` so resolution stays deterministic and testable.
pub fn resolve_transpile_connection<F>(
    config: &Config,
    env: F,
) -> Result<TranspileConnection, String>
where
    F: Fn(&str) -> Option<String>,
{
    let engine = config
        .engines
        .iter()
        .find(|engine| engine.id == config.selected_engine_id)
        .ok_or_else(|| "Selected transpile engine is missing from config.".to_string())?;

    let override_value = |nvidia: &str, nim: &str, configured: &str| {
        env(nvidia)
            .or_else(|| env(nim))
            .unwrap_or_else(|| configured.to_string())
    };
    let base_url = override_value("NVIDIA_BASE_URL", "NIM_BASE_URL", engine.base_url.as_str());

    Ok(TranspileConnection {
        provider: engine.provider.clone(),
        base_url: if base_url.trim().is_empty() {
            NVIDIA_NIM_BASE_URL.to_string()
        } else {
            base_url
        },
        api_key: override_value("NVIDIA_API_KEY", "NIM_API_KEY", engine.api_key.as_str()),
        model: override_value("NVIDIA_MODEL", "NIM_MODEL", engine.model.as_str()),
    })
}

/// Build the existing OpenAI chat-completions payload for one transpile call.
pub fn build_transpile_payload(
    connection: &TranspileConnection,
    source: &str,
    backend: GeometryBackend,
) -> serde_json::Value {
    let (system, user) = build_transpile_messages(source, backend);
    json!({
        "model": connection.model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ],
        "temperature": 0.2,
    })
}

/// Send one transpile request through the shared OpenAI-compatible transport.
/// No dedicated HTTP client exists here; callers provide their established client.
pub async fn transpile_via_openai_compatible(
    client: &reqwest::Client,
    connection: &TranspileConnection,
    source: &str,
    backend: GeometryBackend,
) -> Result<String, String> {
    if connection.api_key.trim().is_empty() {
        return Err("No API key configured for CAD transpile.".to_string());
    }
    if connection.model.trim().is_empty() {
        return Err("No model configured for CAD transpile.".to_string());
    }

    let url = openai_chat_completions_url(&connection.base_url);
    let payload = build_transpile_payload(connection, source, backend);
    let (status, body) = send_openai_request(client, &url, &connection.api_key, &payload).await?;
    if !status.is_success() {
        return Err(format!(
            "OpenAI-compatible transpile error {status}: {body}"
        ));
    }
    let response: serde_json::Value = serde_json::from_str(&body)
        .map_err(|error| format!("Parse transpile response: {error}"))?;
    let source = strip_code_fence(&extract_openai_message_content(&response)?);
    if source.is_empty() {
        return Err("Transpile response contained no Ecky source.".to_string());
    }
    Ok(source)
}

fn openai_chat_completions_url(base_url: &str) -> String {
    let normalized = base_url.trim().trim_end_matches('/');
    if normalized.ends_with("/chat/completions") {
        normalized.to_string()
    } else if normalized.ends_with("/responses") {
        format!(
            "{}/chat/completions",
            normalized.trim_end_matches("/responses")
        )
    } else if normalized.ends_with("/models") {
        format!(
            "{}/chat/completions",
            normalized.trim_end_matches("/models")
        )
    } else {
        format!("{normalized}/chat/completions")
    }
}

/// Run compile, render, structural/authored verification, optional internal
/// source parity, and a capped diagnostic repair loop. This function never
/// commits; it only reports whether a caller may commit the final source.
pub fn run_transpile_gate<R, F>(
    initial_source: String,
    requirements: &DialogueRequirements,
    max_repair_attempts: u32,
    source_measurement: Option<&GeometryMeasurement>,
    parity_tolerance: GeometryParityTolerance,
    runtime: &mut R,
    mut repair: F,
) -> TranspileGateOutcome
where
    R: TranspileGateRuntime,
    F: FnMut(&TranspileRepairRequest) -> Result<String, String>,
{
    let mut source = initial_source;
    let mut attempts = 0_u32;
    let mut repairs = 0_u32;

    loop {
        attempts += 1;
        let mut parity = GeometryParityResult::NotRequested;
        let failure = match runtime.compile_and_render(&source) {
            Err(diagnostic) => Some(diagnostic),
            Ok(rendered) => match runtime.verify_generated_model(&rendered.model_id) {
                Err(diagnostic) => Some(diagnostic),
                Ok(verification) if !verification.passed => {
                    Some(structural_verification_diagnostic(&verification))
                }
                Ok(verification)
                    if !requirements.as_slice().is_empty()
                        && verification.authored_verify_checks.is_empty() =>
                {
                    Some(
                        "Dialogue requirements are not backed by an executed authored `(verify ...)` clause."
                            .to_string(),
                    )
                }
                Ok(_) => {
                    parity = compare_geometry_parity(
                        source_measurement,
                        rendered.measurement.as_ref(),
                        parity_tolerance,
                    );
                    match &parity {
                        GeometryParityResult::Failed { diagnostic, .. } => {
                            Some(diagnostic.clone())
                        }
                        GeometryParityResult::NotRequested
                        | GeometryParityResult::Passed { .. } => None,
                    }
                }
            },
        };

        let Some(diagnostic) = failure else {
            return TranspileGateOutcome {
                source,
                ready_to_commit: true,
                attempts,
                diagnostic: None,
                parity,
            };
        };

        if repairs >= max_repair_attempts {
            return TranspileGateOutcome {
                source,
                ready_to_commit: false,
                attempts,
                diagnostic: Some(diagnostic),
                parity,
            };
        }

        let request = TranspileRepairRequest {
            failed_source: source.clone(),
            diagnostic: diagnostic.clone(),
            prompt: requirements.repair_prompt(&diagnostic),
            repair_number: repairs + 1,
        };
        match repair(&request) {
            Ok(repaired_source) => {
                source = strip_code_fence(&repaired_source);
                repairs += 1;
            }
            Err(repair_error) => {
                return TranspileGateOutcome {
                    source,
                    ready_to_commit: false,
                    attempts,
                    diagnostic: Some(format!(
                        "{diagnostic}\nRepair request {} failed: {repair_error}",
                        repairs + 1
                    )),
                    parity,
                };
            }
        }
    }
}

fn structural_verification_diagnostic(
    verification: &crate::contracts::StructuralVerificationResult,
) -> String {
    let mut lines = Vec::new();
    if !verification.summary.trim().is_empty() {
        lines.push(verification.summary.trim().to_string());
    }
    lines.extend(
        verification
            .issues
            .iter()
            .map(|issue| format!("{}: {}", issue.code, issue.message)),
    );
    lines.extend(
        verification
            .authored_verify_checks
            .iter()
            .filter(|check| {
                check.status == crate::contracts::AuthoredVerifyCheckStatus::Error
                    || (check.status == crate::contracts::AuthoredVerifyCheckStatus::Failed
                        && check.severity == crate::contracts::AuthoredVerifySeverity::Error)
            })
            .map(|check| format!("authored verify {}: {}", check.tag, check.message)),
    );
    if lines.is_empty() {
        "Structural or authored verification failed without a diagnostic.".to_string()
    } else {
        lines.join("\n")
    }
}

fn compare_geometry_parity(
    source: Option<&GeometryMeasurement>,
    rendered: Option<&GeometryMeasurement>,
    tolerance: GeometryParityTolerance,
) -> GeometryParityResult {
    let Some(source) = source else {
        return GeometryParityResult::NotRequested;
    };
    let Some(rendered) = rendered else {
        return GeometryParityResult::Failed {
            diagnostic: "Source parity failed: rendered bbox/volume measurement is unavailable."
                .to_string(),
            bbox_relative_error: None,
            volume_relative_error: None,
        };
    };

    let source_size = bounds_size(&source.bbox);
    let rendered_size = bounds_size(&rendered.bbox);
    let bbox_error = [
        relative_error(source_size[0], rendered_size[0]),
        relative_error(source_size[1], rendered_size[1]),
        relative_error(source_size[2], rendered_size[2]),
    ];
    let volume_error = relative_error(source.volume, rendered.volume);
    let bbox_tolerance = tolerance.bbox_relative.max(0.0);
    let volume_tolerance = tolerance.volume_relative.max(0.0);
    let mut failures = Vec::new();
    for (axis, error) in ["x", "y", "z"].into_iter().zip(bbox_error) {
        if !error.is_finite() || error > bbox_tolerance {
            failures.push(format!(
                "bbox {axis} relative error {error:.6} exceeds {bbox_tolerance:.6}"
            ));
        }
    }
    if !volume_error.is_finite() || volume_error > volume_tolerance {
        failures.push(format!(
            "volume relative error {volume_error:.6} exceeds {volume_tolerance:.6}"
        ));
    }

    if failures.is_empty() {
        GeometryParityResult::Passed {
            bbox_relative_error: bbox_error,
            volume_relative_error: volume_error,
        }
    } else {
        GeometryParityResult::Failed {
            diagnostic: format!("Source parity failed: {}", failures.join("; ")),
            bbox_relative_error: Some(bbox_error),
            volume_relative_error: Some(volume_error),
        }
    }
}

fn bounds_size(bounds: &crate::contracts::ManifestBounds) -> [f64; 3] {
    [
        bounds.x_max - bounds.x_min,
        bounds.y_max - bounds.y_min,
        bounds.z_max - bounds.z_min,
    ]
}

fn relative_error(expected: f64, actual: f64) -> f64 {
    if !expected.is_finite() || !actual.is_finite() {
        return f64::INFINITY;
    }
    let scale = expected.abs();
    if scale <= f64::EPSILON {
        if actual.abs() <= f64::EPSILON {
            0.0
        } else {
            f64::INFINITY
        }
    } else {
        (actual - expected).abs() / scale
    }
}

/// OpenSCAD is already model-readable text. Decode its bytes without changing
/// them; the caller then hands the resulting text to `build_transpile_messages`.
/// Other source-format dispatch remains outside this adapter.
pub fn adapt_openscad_source(path: &Path, source: &[u8]) -> Result<String, String> {
    let is_openscad = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("scad"));
    if !is_openscad {
        return Err(format!(
            "OpenSCAD adapter requires a .scad source, got '{}'",
            path.display()
        ));
    }

    std::str::from_utf8(source)
        .map(str::to_owned)
        .map_err(|error| format!("read OpenSCAD source '{}': {error}", path.display()))
}

/// Strip a single Markdown code fence if the model wrapped its reply in one,
/// tolerating an optional language tag (` ```scheme `) and a trailing fence.
pub fn strip_code_fence(reply: &str) -> String {
    let trimmed = reply.trim();
    let unfenced = if let Some(rest) = trimmed.strip_prefix("```") {
        // Drop the rest of the opening fence line (an optional language tag).
        let body = rest.split_once('\n').map(|x| x.1).unwrap_or("");
        body.trim().strip_suffix("```").unwrap_or(body).trim()
    } else {
        trimmed
    };
    extract_ecky_model(unfenced).unwrap_or_else(|| unfenced.to_string())
}

/// Return the balanced top-level `(model ...)` expression from a chatty reply.
/// Parentheses inside strings and line comments cannot terminate the model.
fn extract_ecky_model(reply: &str) -> Option<String> {
    let start = reply.find("(model")?;
    let bytes = reply.as_bytes();
    let mut depth = 0_u32;
    let mut string = false;
    let mut escaped = false;
    let mut line_comment = false;

    for (offset, byte) in bytes[start..].iter().enumerate() {
        match *byte {
            b'\n' if line_comment => line_comment = false,
            _ if line_comment => continue,
            b';' if !string => line_comment = true,
            b'\\' if string => escaped = !escaped,
            b'"' if !escaped => string = !string,
            _ => escaped = false,
        }
        if string || line_comment {
            continue;
        }
        match *byte {
            b'(' => depth += 1,
            b')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(reply[start..start + offset + 1].trim().to_string());
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cad_source_adapters::{
        adapt_cad_source, CadSourceCommand, CadSourceCommandOutput, CadSourceCommandRunner,
    };
    use crate::contracts::Config;
    use serde_json::json;
    use std::path::Path;

    struct FakeFreecadRunner {
        commands: std::cell::RefCell<Vec<CadSourceCommand>>,
        output: Result<CadSourceCommandOutput, String>,
    }

    impl Default for FakeFreecadRunner {
        fn default() -> Self {
            Self {
                commands: std::cell::RefCell::new(Vec::new()),
                output: Err("fake command output was not configured".to_string()),
            }
        }
    }

    impl CadSourceCommandRunner for FakeFreecadRunner {
        fn run(&self, command: &CadSourceCommand) -> Result<CadSourceCommandOutput, String> {
            self.commands.borrow_mut().push(command.clone());
            self.output.clone()
        }
    }

    fn config(provider: &str, base_url: &str, api_key: &str, model: &str) -> Config {
        serde_json::from_value(json!({
            "engines": [{
                "id": "selected",
                "name": "Selected",
                "provider": provider,
                "baseUrl": base_url,
                "apiKey": api_key,
                "model": model,
            }],
            "selectedEngineId": "selected",
        }))
        .expect("minimal config")
    }

    fn backends() -> [GeometryBackend; 3] {
        [
            GeometryBackend::EckyRust,
            GeometryBackend::Build123d,
            GeometryBackend::Freecad,
        ]
    }

    #[test]
    fn system_is_the_shared_language_reference_and_user_carries_the_source() {
        let source = "// foreign\ncube([1,2,3]);";
        for backend in backends() {
            let (system, user) = build_transpile_messages(source, backend);
            assert_eq!(
                system,
                agent_language_reference(backend),
                "{backend:?} system prompt must be the shared reference verbatim"
            );
            assert!(
                user.contains(source),
                "{backend:?} user must contain the source"
            );
            assert!(
                user.starts_with("Translate the CAD source"),
                "{backend:?} user must lead with the translate instruction"
            );
            // The source is reproduced byte-for-byte after the fixed preamble:
            // `user == TRANSLATE_PREAMBLE + "\n" + source`. This pins both the
            // fixed preamble (1.2) and verbatim source carriage (1.1) — any
            // mutation of the source, any extra wrapping, or a drifted preamble
            // breaks the equality.
            assert_eq!(
                user,
                format!("{TRANSLATE_PREAMBLE}\n{source}"),
                "{backend:?} user must be exactly the fixed preamble + verbatim source"
            );
        }
    }

    #[test]
    fn openscad_adapter_preserves_source_bytes_in_the_transpile_user_message() {
        // Includes CRLF, Unicode, and a trailing newline: adapter must not
        // normalize or wrap OpenSCAD before the model receives it.
        let source = b"// OPENSCAD_SENTINEL \xCE\xBC\r\ncube([1, 2, 3]);\r\n";
        let adapted = adapt_openscad_source(Path::new("fixture.scad"), source).unwrap();
        let (_system, user) = build_transpile_messages(&adapted, GeometryBackend::Build123d);

        assert_eq!(
            user.as_bytes(),
            [TRANSLATE_PREAMBLE.as_bytes(), b"\n", source].concat(),
            ".scad bytes must reach the existing transpile user message unchanged"
        );
    }

    #[test]
    fn fcstd_adapter_uses_fresh_freecadcmd_feature_tree_json_without_emitting_ecky() {
        let runner = FakeFreecadRunner {
            output: Ok(CadSourceCommandOutput {
                success: true,
                stdout: include_str!("../tests/fixtures/cad/transpile/freecad-feature-tree.json")
                    .to_string(),
                stderr: String::new(),
            }),
            ..Default::default()
        };

        let source = adapt_cad_source(Path::new("fixture.FCStd"), b"binary", &runner)
            .expect("FreeCAD feature tree");

        assert!(source.contains("PartDesign::Body"));
        assert!(!source.contains("(model"), "adapter must never emit Ecky");
        let commands = runner.commands.borrow();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].program, "freecadcmd");
        assert!(commands[0].args.iter().any(|arg| arg == "--run"));
        assert!(commands[0].script.contains("doc.Objects"));
        assert!(commands[0].script.contains("json.dump"));
    }

    #[test]
    fn step_adapter_summarizes_bodies_dimensions_and_features_without_ecky() {
        let step = include_bytes!("../tests/fixtures/cad/transpile/simple.step");
        let runner = FakeFreecadRunner::default();

        let source =
            adapt_cad_source(Path::new("fixture.step"), step, &runner).expect("STEP summary");

        assert!(source.contains("bodies: 1"), "{source}");
        assert!(source.contains("dimensions: x=10, y=20, z=30"), "{source}");
        assert!(source.contains("advanced_faces=1"), "{source}");
        assert!(!source.contains("(model"), "adapter must never emit Ecky");
        assert!(
            runner.commands.borrow().is_empty(),
            "STEP must not invoke FreeCAD"
        );
    }

    #[test]
    fn brep_adapter_summarizes_topology_and_coordinate_extent_without_ecky() {
        let brep = include_bytes!("../tests/fixtures/cad/transpile/simple.brep");
        let runner = FakeFreecadRunner::default();

        let source =
            adapt_cad_source(Path::new("fixture.brep"), brep, &runner).expect("BREP summary");

        assert!(source.contains("bodies: 1"), "{source}");
        assert!(source.contains("dimensions: x=4, y=5, z=6"), "{source}");
        assert!(source.contains("faces=1, edges=1, vertices=2"), "{source}");
        assert!(!source.contains("(model"), "adapter must never emit Ecky");
    }

    #[test]
    fn preamble_carries_the_semantic_ask_and_portability_rule() {
        let (_system, user) = build_transpile_messages("x", GeometryBackend::Build123d);
        for needle in [
            "(params",
            "repeat-union",
            "regular-polygon",
            "(verify",
            "ONLY Ecky",
        ] {
            assert!(user.contains(needle), "preamble missing `{needle}`");
        }
    }

    #[test]
    fn strip_code_fence_handles_fenced_and_bare_replies() {
        let bare = "(model (part p (box 1 1 1)))";
        assert_eq!(strip_code_fence(bare), bare);

        let fenced = "```scheme\n(model (part p (box 1 1 1)))\n```";
        assert_eq!(strip_code_fence(fenced), "(model (part p (box 1 1 1)))");

        let fenced_no_lang = "```\n(model)\n```";
        assert_eq!(strip_code_fence(fenced_no_lang), "(model)");

        let with_prose_then_fence = "  ```ecky\n(a)\n(b)\n```  ";
        assert_eq!(strip_code_fence(with_prose_then_fence), "(a)\n(b)");
    }

    #[test]
    fn resolve_uses_selected_config_engine_then_documented_nim_environment_overrides() {
        let configured = config(
            "nim",
            "https://configured.example/v1",
            "configured-key",
            "configured-model",
        );
        let resolved = resolve_transpile_connection(&configured, |name| match name {
            "NIM_API_KEY" => Some("nim-key".to_string()),
            "NIM_BASE_URL" => Some("https://nim.example/v1".to_string()),
            "NIM_MODEL" => Some("nim-model".to_string()),
            _ => None,
        })
        .expect("resolved connection");

        assert_eq!(resolved.provider, "nim");
        assert_eq!(resolved.api_key, "nim-key");
        assert_eq!(resolved.base_url, "https://nim.example/v1");
        assert_eq!(resolved.model, "nim-model");
    }

    #[test]
    fn resolve_defaults_empty_configured_base_url_to_nvidia_nim() {
        let resolved = resolve_transpile_connection(&config("nim", "", "key", "model"), |_| None)
            .expect("resolved connection");

        assert_eq!(resolved.base_url, NVIDIA_NIM_BASE_URL);
    }

    #[test]
    fn transpile_payload_uses_openai_messages_with_the_resolved_model() {
        let connection = TranspileConnection {
            provider: "nim".to_string(),
            base_url: "https://integrate.api.nvidia.com/v1".to_string(),
            api_key: "key".to_string(),
            model: "nim-model".to_string(),
        };
        let payload =
            build_transpile_payload(&connection, "cube([1,2,3]);", GeometryBackend::Build123d);

        assert_eq!(payload["model"], "nim-model");
        assert_eq!(payload["messages"][0]["role"], "system");
        assert_eq!(payload["messages"][1]["role"], "user");
        assert!(payload["messages"][1]["content"]
            .as_str()
            .expect("user content")
            .contains("cube([1,2,3]);"));
    }

    #[test]
    fn strip_code_fence_removes_leading_and_trailing_prose_around_ecky_model() {
        let reply = "Here is the transpiled model:\n```ecky\n(model (part body (box 1 2 3)))\n```\nThis preserves the dimensions.";
        assert_eq!(strip_code_fence(reply), "(model (part body (box 1 2 3)))");
    }

    struct FakeGateRuntime {
        renders: std::collections::VecDeque<Result<TranspileRender, String>>,
        verifications: std::collections::VecDeque<
            Result<crate::contracts::StructuralVerificationResult, String>,
        >,
        events: Vec<String>,
    }

    impl TranspileGateRuntime for FakeGateRuntime {
        fn compile_and_render(&mut self, source: &str) -> Result<TranspileRender, String> {
            self.events.push(format!("render:{source}"));
            self.renders.pop_front().expect("configured render")
        }

        fn verify_generated_model(
            &mut self,
            model_id: &str,
        ) -> Result<crate::contracts::StructuralVerificationResult, String> {
            self.events.push(format!("verify:{model_id}"));
            self.verifications
                .pop_front()
                .expect("configured verification")
        }
    }

    fn bounds(size: [f64; 3]) -> crate::contracts::ManifestBounds {
        crate::contracts::ManifestBounds {
            x_min: 0.0,
            y_min: 0.0,
            z_min: 0.0,
            x_max: size[0],
            y_max: size[1],
            z_max: size[2],
        }
    }

    fn verification(
        passed: bool,
        diagnostic: &str,
    ) -> crate::contracts::StructuralVerificationResult {
        crate::contracts::StructuralVerificationResult {
            passed,
            summary: diagnostic.to_string(),
            issues: if passed {
                Vec::new()
            } else {
                vec![crate::contracts::StructuralIssue {
                    code: "VERIFY_RED".to_string(),
                    message: diagnostic.to_string(),
                    part_id: None,
                    numeric_payload: None,
                    diagnostic_context: None,
                }]
            },
            authored_verify_checks: Vec::new(),
            metrics: crate::contracts::StructuralMetrics {
                part_count: 1,
                model_stl_size_bytes: None,
                model_stl_triangle_count: None,
                model_stl_component_count: Some(1),
                model_stl_non_manifold_edge_count: Some(0),
                model_stl_overhang_triangle_count: None,
                model_stl_overhang_ratio: None,
                total_volume: Some(6_000.0),
                total_area: None,
                bbox: Some(bounds([10.0, 20.0, 30.0])),
            },
            verifier_status: crate::contracts::VerifierStatus::OkRustOnly,
            verifier_source: None,
        }
    }

    fn runtime_for(
        render: Result<TranspileRender, String>,
        verify: Option<Result<crate::contracts::StructuralVerificationResult, String>>,
    ) -> FakeGateRuntime {
        FakeGateRuntime {
            renders: [render].into_iter().collect(),
            verifications: verify.into_iter().collect(),
            events: Vec::new(),
        }
    }

    #[test]
    fn gate_compiles_renders_then_runs_existing_generated_model_verification() {
        let mut runtime = runtime_for(
            Ok(TranspileRender {
                model_id: "rendered-1".to_string(),
                measurement: None,
            }),
            Some(Ok(verification(true, "green"))),
        );
        let outcome = run_transpile_gate(
            "(model (part body (box 10 20 30)))".to_string(),
            &DialogueRequirements::default(),
            0,
            None,
            GeometryParityTolerance::default(),
            &mut runtime,
            |_| panic!("green result must not repair"),
        );

        assert!(outcome.ready_to_commit);
        assert_eq!(outcome.attempts, 1);
        assert_eq!(
            runtime.events,
            [
                "render:(model (part body (box 10 20 30)))",
                "verify:rendered-1"
            ]
        );
    }

    #[test]
    fn dialogue_requirements_accumulate_and_demand_matching_verify_clauses() {
        let mut requirements = DialogueRequirements::default();
        requirements.record("ears should be separate");
        requirements.record("head width must stay 12 mm");
        requirements.record(" ears should be separate ");

        let prompt = requirements.repair_prompt("VERIFY_RED: connected components = 1");
        assert_eq!(requirements.as_slice().len(), 2);
        assert!(prompt.contains("ears should be separate"), "{prompt}");
        assert!(prompt.contains("head width must stay 12 mm"), "{prompt}");
        assert!(
            prompt.contains("matching `(verify ...)` clause"),
            "{prompt}"
        );
        assert!(
            prompt.contains("VERIFY_RED: connected components = 1"),
            "{prompt}"
        );
    }

    #[test]
    fn dialogue_requirement_without_executed_authored_check_stays_red() {
        let mut requirements = DialogueRequirements::default();
        requirements.record("ears should be separate");
        let mut runtime = runtime_for(
            Ok(TranspileRender {
                model_id: "missing-authored-check".to_string(),
                measurement: None,
            }),
            Some(Ok(verification(true, "structurally green"))),
        );
        let outcome = run_transpile_gate(
            "(model (part body (box 1 2 3)))".to_string(),
            &requirements,
            0,
            None,
            GeometryParityTolerance::default(),
            &mut runtime,
            |_| panic!("repair cap is zero"),
        );

        assert!(!outcome.ready_to_commit);
        assert!(outcome
            .diagnostic
            .as_deref()
            .is_some_and(|diagnostic| diagnostic.contains("executed authored `(verify ...)`")));
    }

    #[test]
    fn compiler_diagnostic_drives_bounded_repair_then_green_verification() {
        let mut runtime = FakeGateRuntime {
            renders: [
                Err("compile: unknown operation bx".to_string()),
                Ok(TranspileRender {
                    model_id: "rendered-fixed".to_string(),
                    measurement: None,
                }),
            ]
            .into_iter()
            .collect(),
            verifications: [Ok(verification(true, "green"))].into_iter().collect(),
            events: Vec::new(),
        };
        let mut repair_prompts = Vec::new();
        let outcome = run_transpile_gate(
            "(model (part body (bx 1 2 3)))".to_string(),
            &DialogueRequirements::default(),
            1,
            None,
            GeometryParityTolerance::default(),
            &mut runtime,
            |request| {
                repair_prompts.push(request.prompt.clone());
                Ok("(model (part body (box 1 2 3)))".to_string())
            },
        );

        assert!(outcome.ready_to_commit);
        assert_eq!(outcome.attempts, 2);
        assert_eq!(repair_prompts.len(), 1);
        assert!(repair_prompts[0].contains("compile: unknown operation bx"));
    }

    #[test]
    fn capped_red_verification_reports_diagnostic_and_never_becomes_commit_ready() {
        let mut runtime = FakeGateRuntime {
            renders: ["red-1", "red-2"]
                .into_iter()
                .map(|model_id| {
                    Ok(TranspileRender {
                        model_id: model_id.to_string(),
                        measurement: None,
                    })
                })
                .collect(),
            verifications: [
                Ok(verification(false, "non-manifold edge count = 2")),
                Ok(verification(false, "non-manifold edge count = 1")),
            ]
            .into_iter()
            .collect(),
            events: Vec::new(),
        };
        let outcome = run_transpile_gate(
            "red source".to_string(),
            &DialogueRequirements::default(),
            1,
            None,
            GeometryParityTolerance::default(),
            &mut runtime,
            |_| Ok("still red source".to_string()),
        );

        assert!(!outcome.ready_to_commit);
        assert_eq!(outcome.attempts, 2);
        assert!(outcome
            .diagnostic
            .as_deref()
            .is_some_and(|value| value.contains("non-manifold edge count = 1")));
    }

    #[test]
    fn measurable_source_bbox_and_volume_mismatch_fail_internal_parity_gate() {
        let source = GeometryMeasurement {
            bbox: bounds([10.0, 20.0, 30.0]),
            volume: 6_000.0,
        };
        let rendered = GeometryMeasurement {
            bbox: bounds([20.0, 20.0, 30.0]),
            volume: 12_000.0,
        };
        let mut runtime = runtime_for(
            Ok(TranspileRender {
                model_id: "oversized-head".to_string(),
                measurement: Some(rendered),
            }),
            Some(Ok(verification(true, "structurally green"))),
        );
        let outcome = run_transpile_gate(
            "structurally green but wrong size".to_string(),
            &DialogueRequirements::default(),
            0,
            Some(&source),
            GeometryParityTolerance {
                bbox_relative: 0.01,
                volume_relative: 0.02,
            },
            &mut runtime,
            |_| panic!("repair cap is zero"),
        );

        assert!(!outcome.ready_to_commit);
        assert!(matches!(
            outcome.parity,
            GeometryParityResult::Failed { .. }
        ));
        let diagnostic = outcome.diagnostic.expect("parity diagnostic");
        assert!(diagnostic.contains("bbox x"), "{diagnostic}");
        assert!(diagnostic.contains("volume"), "{diagnostic}");
    }

    #[test]
    fn ui_tier_without_source_measurement_skips_parity() {
        let mut runtime = runtime_for(
            Ok(TranspileRender {
                model_id: "consumer-render".to_string(),
                measurement: None,
            }),
            Some(Ok(verification(true, "green"))),
        );
        let outcome = run_transpile_gate(
            "source".to_string(),
            &DialogueRequirements::default(),
            0,
            None,
            GeometryParityTolerance::default(),
            &mut runtime,
            |_| panic!("green result must not repair"),
        );

        assert!(outcome.ready_to_commit);
        assert_eq!(outcome.parity, GeometryParityResult::NotRequested);
    }
}
