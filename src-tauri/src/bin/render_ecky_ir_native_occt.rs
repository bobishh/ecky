use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ecky_cad_lib::ecky_cad_host::direct_occt_executor;
use ecky_cad_lib::ecky_cad_host::direct_occt_sdk;
use ecky_cad_lib::ecky_scheme::try_compile_to_core_program;
use ecky_cad_lib::models::{DesignParams, PathResolver};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let source = std::fs::read_to_string(&args.input)
        .map_err(|err| format!("Failed to read `{}`: {err}", args.input.display()))?;
    let program = match try_compile_to_core_program(&source) {
        Some(result) => result.map_err(|err| err.to_string())?,
        None => return Err("Source is not compileable `.ecky` model syntax.".to_string()),
    };
    let params: DesignParams = if let Some(params_path) = args.params {
        let raw = std::fs::read_to_string(&params_path)
            .map_err(|err| format!("Failed to read `{}`: {err}", params_path.display()))?;
        serde_json::from_str(&raw).map_err(|err| {
            format!(
                "Failed to parse params JSON `{}`: {err}",
                params_path.display()
            )
        })?
    } else {
        DesignParams::new()
    };

    std::fs::create_dir_all(&args.out_dir)
        .map_err(|err| format!("Failed to create `{}`: {err}", args.out_dir.display()))?;

    let repo_root = repo_root()?;
    let resolver = CliPathResolver {
        root: repo_root.clone(),
    };
    let runtime_root = args
        .runtime_root
        .unwrap_or_else(|| direct_occt_sdk::bundled_occt_runtime_root_from_repo(&repo_root));
    let fallback_runtime_root =
        direct_occt_sdk::bundled_build123d_runtime_root_from_repo(&repo_root);
    let layout = if runtime_root.exists() {
        direct_occt_sdk::inspect_build123d_ocp_runtime(&runtime_root)
    } else {
        direct_occt_sdk::inspect_build123d_ocp_runtime(&fallback_runtime_root)
    };

    let outcome = direct_occt_executor::export_core_program_step_stl_with_params_runner_first(
        &program,
        &params,
        &layout,
        &args.out_dir,
        &resolver,
    )
    .map_err(|err| err.to_string())?;

    match outcome {
        direct_occt_sdk::NativeExportOutcome::Exported {
            step_path,
            stl_path,
        } => {
            println!("step={}", step_path.display());
            println!("stl={}", stl_path.display());
            Ok(())
        }
        direct_occt_sdk::NativeExportOutcome::Blocked { blockers } => Err(format!(
            "Direct OCCT export blocked: {}",
            blockers.join("; ")
        )),
    }
}

#[derive(Debug)]
struct Args {
    input: PathBuf,
    out_dir: PathBuf,
    params: Option<PathBuf>,
    runtime_root: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut input = None;
    let mut out_dir = None;
    let mut params = None;
    let mut runtime_root = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out-dir" => out_dir = Some(next_path(&mut args, "--out-dir")?),
            "--params" => params = Some(next_path(&mut args, "--params")?),
            "--runtime-root" => runtime_root = Some(next_path(&mut args, "--runtime-root")?),
            "--help" | "-h" => return Err(usage().to_string()),
            _ if input.is_none() => input = Some(PathBuf::from(arg)),
            _ => return Err(usage().to_string()),
        }
    }

    Ok(Args {
        input: input.ok_or_else(|| usage().to_string())?,
        out_dir: out_dir.ok_or_else(|| usage().to_string())?,
        params,
        runtime_root,
    })
}

fn next_path(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<PathBuf, String> {
    args.next()
        .map(PathBuf::from)
        .ok_or_else(|| format!("`{flag}` needs a value.\n{}", usage()))
}

fn usage() -> &'static str {
    "Usage: render_ecky_ir_native_occt <input.ecky> --out-dir <dir> [--params params.json] [--runtime-root runtime/occt]"
}

fn repo_root() -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|err| err.to_string())?;
    for candidate in cwd.ancestors() {
        if candidate.join("src-tauri/Cargo.toml").is_file() {
            return Ok(candidate.to_path_buf());
        }
        if candidate.file_name().and_then(|name| name.to_str()) == Some("src-tauri") {
            if let Some(parent) = candidate.parent() {
                return Ok(parent.to_path_buf());
            }
        }
    }
    Err("Could not resolve repository root.".to_string())
}

struct CliPathResolver {
    root: PathBuf,
}

impl PathResolver for CliPathResolver {
    fn app_config_dir(&self) -> PathBuf {
        self.root.join("tmp").join("cli-app-config")
    }

    fn app_data_dir(&self) -> PathBuf {
        self.root.join("tmp").join("cli-app-data")
    }

    fn resource_path(&self, path: &str) -> Option<PathBuf> {
        let relative = Path::new(path);
        [
            self.root.join(".dist").join(relative),
            self.root.join("src-tauri/target/debug").join(relative),
            self.root.join("src-tauri/target/release").join(relative),
            self.root.join(relative),
        ]
        .into_iter()
        .find(|candidate| candidate.exists())
    }
}
