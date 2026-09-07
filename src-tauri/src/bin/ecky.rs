use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ecky_cad_lib::contracts::{ArtifactBundle, DesignParams, GeometryBackend, ParamValue};
use ecky_cad_lib::models::PathResolver;

const USAGE: &str = "Usage:\n  ecky check <input>\n  ecky lower --backend freecad|build123d <input> [--out <path>]\n  ecky render --backend <freecad|native> <input> (--stl <path>|--bundle-dir <dir>) [--step <path>] [--param key=value]... [--params file.json] [--json]\n\nCommands:\n  check   Validate Ecky source.\n  lower   Lower Ecky source without rendering.\n  render  Render Ecky source into requested artifacts.\n";

#[derive(Debug)]
enum CliError {
    Usage(String),
    Check(String),
    Lower(String),
    Render(String),
    Write(String),
}

impl CliError {
    fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) => 2,
            Self::Check(_) => 3,
            Self::Lower(_) => 4,
            Self::Render(_) => 5,
            Self::Write(_) => 6,
        }
    }

    fn message(&self) -> &str {
        match self {
            Self::Usage(message)
            | Self::Check(message)
            | Self::Lower(message)
            | Self::Render(message)
            | Self::Write(message) => message,
        }
    }
}

struct CliResolver {
    root: PathBuf,
}

impl CliResolver {
    fn new() -> Result<Self, CliError> {
        let root = env::temp_dir().join(format!("ecky-cli-{}", std::process::id()));
        fs::create_dir_all(&root).map_err(|error| CliError::Write(error.to_string()))?;
        Ok(Self { root })
    }
}

impl PathResolver for CliResolver {
    fn app_config_dir(&self) -> PathBuf {
        self.root.join("config")
    }

    fn app_data_dir(&self) -> PathBuf {
        self.root.join("data")
    }

    fn resource_path(&self, path: &str) -> Option<PathBuf> {
        env::var_os("ECKY_RESOURCE_DIR").map(|root| PathBuf::from(root).join(path))
    }
}

fn usage_error(message: impl Into<String>) -> CliError {
    CliError::Usage(format!("{}\n\n{}", message.into(), USAGE.trim_end()))
}

fn read_source(path: &str, failure: fn(String) -> CliError) -> Result<String, CliError> {
    fs::read_to_string(path).map_err(|error| failure(error.to_string()))
}

fn check(args: &[String]) -> Result<(), CliError> {
    if args.len() != 1 || args[0].starts_with('-') {
        return Err(usage_error("check requires exactly one input path"));
    }
    let source = read_source(&args[0], CliError::Check)?;
    let program = ecky_cad_lib::ecky_scheme::compile_to_core_program(&source)
        .map_err(|error| CliError::Check(error.render_with_source(&source)))?;
    println!("check: ok");
    println!("parts: {}", program.parts.len());
    println!("params: {}", program.parameters.len());
    Ok(())
}

fn lower(args: &[String]) -> Result<(), CliError> {
    let mut backend = None;
    let mut input = None;
    let mut output = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--backend" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| usage_error("--backend requires a value"))?;
                if backend.replace(value.as_str()).is_some() {
                    return Err(usage_error("--backend may only be provided once"));
                }
            }
            "--out" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| usage_error("--out requires a path"))?;
                if output.replace(PathBuf::from(value)).is_some() {
                    return Err(usage_error("--out may only be provided once"));
                }
            }
            value if value.starts_with('-') => {
                return Err(usage_error(format!("unknown flag: {value}")))
            }
            value => {
                if input.replace(value).is_some() {
                    return Err(usage_error("lower requires exactly one input path"));
                }
            }
        }
        index += 1;
    }

    let backend = backend.ok_or_else(|| usage_error("lower requires --backend"))?;
    let input = input.ok_or_else(|| usage_error("lower requires an input path"))?;
    if !matches!(backend, "freecad" | "build123d") {
        return Err(usage_error(format!("unsupported backend: {backend}")));
    }
    let source = read_source(input, CliError::Lower)?;
    let lowered = match backend {
        "freecad" => ecky_cad_lib::ecky_ir::lower_to_freecad(&source),
        "build123d" => ecky_cad_lib::ecky_ir::lower_to_freecad(&source).map(|lowered| {
            let mut output = String::from("from build123d import *\n");
            output.push_str(&lowered);
            output
        }),
        _ => unreachable!("validated backend"),
    }
    .map_err(|error| CliError::Lower(error.to_string()))?;

    if let Some(path) = output {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|error| CliError::Write(error.to_string()))?;
        }
        fs::write(path, lowered).map_err(|error| CliError::Write(error.to_string()))?;
    } else {
        print!("{lowered}");
        io::stdout()
            .flush()
            .map_err(|error| CliError::Write(error.to_string()))?;
    }
    Ok(())
}

struct RenderArgs {
    backend: GeometryBackend,
    backend_name: String,
    input: String,
    stl: Option<PathBuf>,
    step: Option<PathBuf>,
    bundle_dir: Option<PathBuf>,
    parameters: DesignParams,
    json: bool,
}

fn parse_param(token: &str) -> Result<(String, ParamValue), CliError> {
    let (key, raw_value) = token
        .split_once('=')
        .ok_or_else(|| usage_error("--param requires key=value"))?;
    let key = key.trim();
    if key.is_empty() {
        return Err(usage_error("--param requires a non-empty key"));
    }
    let value = match raw_value {
        "true" => ParamValue::Boolean(true),
        "false" => ParamValue::Boolean(false),
        _ => raw_value
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite())
            .map(ParamValue::Number)
            .unwrap_or_else(|| ParamValue::String(raw_value.to_string())),
    };
    Ok((key.to_string(), value))
}

fn read_json_params(path: &Path) -> Result<DesignParams, CliError> {
    let raw = fs::read_to_string(path).map_err(|error| CliError::Usage(error.to_string()))?;
    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|error| CliError::Usage(error.to_string()))?;
    let object = value
        .as_object()
        .ok_or_else(|| usage_error("--params JSON root must be an object"))?;
    object
        .iter()
        .map(|(key, value)| {
            let value = match value {
                serde_json::Value::Bool(value) => ParamValue::Boolean(*value),
                serde_json::Value::Number(value) => value
                    .as_f64()
                    .filter(|value| value.is_finite())
                    .map(ParamValue::Number)
                    .ok_or_else(|| {
                        usage_error(format!("unsupported JSON value for parameter: {key}"))
                    })?,
                serde_json::Value::String(value) => ParamValue::String(value.clone()),
                _ => {
                    return Err(usage_error(format!(
                        "unsupported JSON value for parameter: {key}"
                    )))
                }
            };
            Ok((key.clone(), value))
        })
        .collect()
}

fn parse_render(args: &[String]) -> Result<RenderArgs, CliError> {
    let mut backend = None;
    let mut input = None;
    let mut stl = None;
    let mut step = None;
    let mut bundle_dir = None;
    let mut params_file = None;
    let mut param_tokens = Vec::new();
    let mut json = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--backend" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| usage_error("--backend requires a value"))?;
                if backend.replace(value.to_string()).is_some() {
                    return Err(usage_error("--backend may only be provided once"));
                }
            }
            "--stl" | "--step" | "--bundle-dir" | "--params" | "--param" => {
                let flag = args[index].as_str();
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| usage_error(format!("{flag} requires a value")))?;
                match flag {
                    "--stl" => {
                        if stl.replace(PathBuf::from(value)).is_some() {
                            return Err(usage_error("--stl may only be provided once"));
                        }
                    }
                    "--step" => {
                        if step.replace(PathBuf::from(value)).is_some() {
                            return Err(usage_error("--step may only be provided once"));
                        }
                    }
                    "--bundle-dir" => {
                        if bundle_dir.replace(PathBuf::from(value)).is_some() {
                            return Err(usage_error("--bundle-dir may only be provided once"));
                        }
                    }
                    "--params" => {
                        if params_file.replace(PathBuf::from(value)).is_some() {
                            return Err(usage_error("--params may only be provided once"));
                        }
                    }
                    "--param" => param_tokens.push(value.clone()),
                    _ => unreachable!("matched render value flag"),
                }
            }
            "--json" => {
                if json {
                    return Err(usage_error("--json may only be provided once"));
                }
                json = true;
            }
            value if value.starts_with('-') => {
                return Err(usage_error(format!("unknown flag: {value}")))
            }
            value => {
                if input.replace(value.to_string()).is_some() {
                    return Err(usage_error("render requires exactly one input path"));
                }
            }
        }
        index += 1;
    }
    let backend_name = backend.ok_or_else(|| usage_error("render requires --backend"))?;
    let backend = match backend_name.as_str() {
        "freecad" => GeometryBackend::Freecad,
        "native" | "direct-occt" | "build123d" => GeometryBackend::EckyRust,
        _ => return Err(usage_error(format!("unsupported backend: {backend_name}"))),
    };
    if stl.is_none() && bundle_dir.is_none() {
        return Err(usage_error("render requires --stl or --bundle-dir"));
    }
    let mut parameters = match params_file {
        Some(path) => read_json_params(&path)?,
        None => DesignParams::new(),
    };
    for token in param_tokens {
        let (key, value) = parse_param(&token)?;
        parameters.insert(key, value);
    }
    Ok(RenderArgs {
        backend,
        backend_name,
        input: input.ok_or_else(|| usage_error("render requires an input path"))?,
        stl,
        step,
        bundle_dir,
        parameters,
        json,
    })
}

fn copy_artifact(source: &Path, destination: &Path) -> Result<(), CliError> {
    if !source.is_file() {
        return Err(CliError::Write(format!(
            "requested artifact missing: {}",
            source.display()
        )));
    }
    if let Some(parent) = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|error| CliError::Write(error.to_string()))?;
    }
    fs::copy(source, destination).map_err(|error| CliError::Write(error.to_string()))?;
    Ok(())
}

fn copy_bundle(bundle: &ArtifactBundle, destination: &Path) -> Result<(), CliError> {
    fs::create_dir_all(destination).map_err(|error| CliError::Write(error.to_string()))?;
    let mut artifacts = vec![
        PathBuf::from(&bundle.model_stl_path),
        PathBuf::from(&bundle.manifest_path),
    ];
    if let Some(path) = &bundle.macro_path {
        artifacts.push(PathBuf::from(path));
    }
    if !bundle.fcstd_path.trim().is_empty() {
        artifacts.push(PathBuf::from(&bundle.fcstd_path));
    }
    artifacts.extend(
        bundle
            .export_artifacts
            .iter()
            .map(|artifact| PathBuf::from(&artifact.path)),
    );
    for artifact in artifacts {
        if artifact.is_file() {
            copy_artifact(
                &artifact,
                &destination.join(artifact.file_name().unwrap_or_default()),
            )?;
        }
    }
    Ok(())
}

fn render_with(
    args: &[String],
    render_backend: impl FnOnce(
        &str,
        &DesignParams,
        GeometryBackend,
        &dyn PathResolver,
    ) -> std::result::Result<
        ArtifactBundle,
        Box<ecky_cad_lib::contracts::AppError>,
    >,
) -> Result<(), CliError> {
    let args = parse_render(args)?;
    let source = read_source(&args.input, CliError::Render)?;
    let resolver = CliResolver::new()?;
    let bundle = render_backend(&source, &args.parameters, args.backend, &resolver)
        .map_err(|error| CliError::Render(error.to_string()))?;
    if let Some(path) = &args.stl {
        copy_artifact(Path::new(&bundle.model_stl_path), path)?;
    }
    if let Some(path) = &args.step {
        let source = bundle
            .export_artifacts
            .iter()
            .find(|artifact| artifact.format.eq_ignore_ascii_case("step"))
            .map(|artifact| Path::new(&artifact.path))
            .ok_or_else(|| {
                CliError::Write("requested STEP artifact missing after render".to_string())
            })?;
        copy_artifact(source, path)?;
    }
    if let Some(path) = &args.bundle_dir {
        copy_bundle(&bundle, path)?;
    }
    if args.json {
        println!(
            "{}",
            serde_json::json!({ "backend": args.backend_name, "modelStlPath": bundle.model_stl_path, "stepPath": bundle.export_artifacts.iter().find(|artifact| artifact.format.eq_ignore_ascii_case("step")).map(|artifact| artifact.path.clone()), "manifestPath": bundle.manifest_path, "contentHash": bundle.content_hash })
        );
    } else {
        println!("render: ok");
    }
    Ok(())
}

fn render(args: &[String]) -> Result<(), CliError> {
    render_with(args, |source, parameters, backend, resolver| {
        ecky_cad_lib::services::render::render_cli_ecky(
            source,
            parameters,
            backend,
            env::var("ECKY_FREECAD_CMD").ok().as_deref(),
            resolver,
        )
        .map_err(Box::new)
    })
}

fn run(args: &[String]) -> Result<(), CliError> {
    match args {
        [] => Err(usage_error("missing command")),
        [command] if command == "--help" || command == "-h" => {
            print!("{USAGE}");
            Ok(())
        }
        [command, rest @ ..] if command == "check" => check(rest),
        [command, rest @ ..] if command == "lower" => lower(rest),
        [command, rest @ ..] if command == "render" => render(rest),
        [command, ..] => Err(usage_error(format!("unknown command: {command}"))),
    }
}

fn main() -> ExitCode {
    let args = env::args().skip(1).collect::<Vec<_>>();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}", error.message());
            ExitCode::from(error.exit_code())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ecky_cad_lib::contracts::{AppError, AppErrorCode};
    use std::sync::atomic::{AtomicU64, Ordering};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = env::temp_dir().join(format!(
                "ecky-cli-bin-{name}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).expect("create test dir");
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn bundle(root: &Path, preview: &Path, step: Option<&Path>) -> ArtifactBundle {
        let manifest = root.join("manifest.json");
        fs::write(&manifest, "{}").expect("write manifest");
        let exports = step
            .map(|path| {
                vec![serde_json::json!({
                    "label": "STEP", "format": "step", "path": path, "role": "primary"
                })]
            })
            .unwrap_or_default();
        serde_json::from_value(serde_json::json!({
            "schemaVersion": 1,
            "modelId": "test-model",
            "sourceKind": "generated",
            "engineKind": "ecky",
            "sourceLanguage": "ecky",
            "geometryBackend": "build123d",
            "contentHash": "hash",
            "artifactVersion": 1,
            "fcstdPath": "",
            "manifestPath": manifest,
            "modelStlPath": preview,
            "exportArtifacts": exports
        }))
        .expect("build artifact bundle")
    }

    fn render_args(input: &Path, extra: &[&str]) -> Vec<String> {
        let mut args = vec![
            "--backend".to_string(),
            "build123d".to_string(),
            input.display().to_string(),
        ];
        args.extend(extra.iter().map(|value| (*value).to_string()));
        args
    }

    #[test]
    fn render_routes_freecad_to_freecad_backend_without_runner() {
        let root = TempDir::new("freecad-route");
        let input = root.0.join("input.ecky");
        let preview = root.0.join("model.stl");
        let output = root.0.join("out.stl");
        fs::write(&input, "(model (part body (box 1 2 3)))").expect("write input");
        fs::write(&preview, "stl").expect("write preview");
        let args = vec![
            "--backend".to_string(),
            "freecad".to_string(),
            input.display().to_string(),
            "--stl".to_string(),
            output.display().to_string(),
        ];

        render_with(&args, |_, _, backend, _| {
            assert_eq!(backend, GeometryBackend::Freecad);
            Ok(bundle(&root.0, &preview, None))
        })
        .expect("freecad route");
        assert_eq!(fs::read_to_string(output).expect("read copied STL"), "stl");
    }

    #[test]
    fn render_routes_direct_occt_to_native_backend_without_runner() {
        let root = TempDir::new("direct-route");
        let input = root.0.join("input.ecky");
        let preview = root.0.join("model.stl");
        let output = root.0.join("out.stl");
        fs::write(&input, "(model (part body (box 1 2 3)))").expect("write input");
        fs::write(&preview, "stl").expect("write preview");
        let args = vec![
            "--backend".to_string(),
            "direct-occt".to_string(),
            input.display().to_string(),
            "--stl".to_string(),
            output.display().to_string(),
        ];

        render_with(&args, |_, _, backend, _| {
            assert_eq!(backend, GeometryBackend::EckyRust);
            Ok(bundle(&root.0, &preview, None))
        })
        .expect("direct OCCT route");
    }

    #[test]
    fn render_copies_step_and_fails_when_requested_artifact_is_missing() {
        let root = TempDir::new("step-copy");
        let input = root.0.join("input.ecky");
        let preview = root.0.join("model.stl");
        let step = root.0.join("model.step");
        let output = root.0.join("out.stl");
        let copied_step = root.0.join("nested/out.step");
        fs::write(&input, "(model (part body (box 1 2 3)))").expect("write input");
        fs::write(&preview, "stl").expect("write preview");
        fs::write(&step, "step").expect("write step");
        let args = render_args(
            &input,
            &[
                "--stl",
                &output.display().to_string(),
                "--step",
                &copied_step.display().to_string(),
            ],
        );
        render_with(&args, |_, _, _, _| {
            Ok(bundle(&root.0, &preview, Some(&step)))
        })
        .expect("copy STEP");
        assert_eq!(
            fs::read_to_string(&copied_step).expect("read copied STEP"),
            "step"
        );

        let missing = root.0.join("missing.stl");
        let missing_args = render_args(&input, &["--stl", &output.display().to_string()]);
        let error = render_with(&missing_args, |_, _, _, _| {
            Ok(bundle(&root.0, &missing, None))
        })
        .expect_err("missing artifact should fail");
        assert_eq!(error.exit_code(), 6);
        assert!(error.message().contains("requested artifact missing"));
    }

    #[test]
    fn render_keeps_raw_backend_error_details() {
        let root = TempDir::new("raw-error");
        let input = root.0.join("input.ecky");
        let output = root.0.join("out.stl");
        fs::write(&input, "(model (part body (box 1 2 3)))").expect("write input");
        let args = render_args(&input, &["--stl", &output.display().to_string()]);

        let error = render_with(&args, |_, _, _, _| {
            Err(Box::new(AppError::with_details(
                AppErrorCode::Render,
                "native runner failed",
                "stderr: unfiltered backend detail",
            )))
        })
        .expect_err("backend error should fail");
        assert_eq!(error.exit_code(), 5);
        assert!(error
            .message()
            .contains("stderr: unfiltered backend detail"));
    }
}
