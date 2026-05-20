//! `ecky` CLI: compile-check and lower `.ecky` sources from the shell.
//!
//! Subcommands:
//! - `ecky check <input.ecky>`: compile to Core IR; silent success, compiler
//!   error on stderr with exit code 1.
//! - `ecky lower --backend <build123d|freecad> <input.ecky> --out <path>`:
//!   lower to the requested backend source and write it to `--out`.

use std::path::PathBuf;
use std::process::ExitCode;

const LOWER_STACK_SIZE: usize = 32 * 1024 * 1024;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("check") => run_check(&args[1..]),
        Some("lower") => run_lower(&args[1..]),
        Some(other) => {
            eprintln!("Unknown subcommand `{other}`.");
            print_usage();
            ExitCode::from(2)
        }
        None => {
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  ecky check <input.ecky>");
    eprintln!("  ecky lower --backend <build123d|freecad> <input.ecky> --out <output>");
}

fn run_check(args: &[String]) -> ExitCode {
    let Some(input) = args.first() else {
        eprintln!("`ecky check` needs an input file.");
        return ExitCode::from(2);
    };
    let source = match std::fs::read_to_string(input) {
        Ok(source) => source,
        Err(err) => {
            eprintln!("Failed to read `{input}`: {err}");
            return ExitCode::from(1);
        }
    };
    // Check through the lowering parser: it validates the full authoring
    // surface the runtime accepts and reports authoring-grade errors.
    let outcome = std::thread::Builder::new()
        .stack_size(LOWER_STACK_SIZE)
        .spawn(move || ecky_cad_lib::ecky_ir::lower_to_build123d(&source).map(|_| ()))
        .expect("spawn check thread")
        .join();
    match outcome {
        Ok(Ok(())) => ExitCode::SUCCESS,
        Ok(Err(err)) => {
            eprintln!("{err}");
            ExitCode::from(1)
        }
        Err(_) => {
            eprintln!("Check panicked.");
            ExitCode::from(1)
        }
    }
}

struct LowerArgs {
    backend: String,
    input: PathBuf,
    out: PathBuf,
}

fn parse_lower_args(args: &[String]) -> Result<LowerArgs, String> {
    let mut backend = None;
    let mut input = None;
    let mut out = None;
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--backend" => {
                backend = Some(
                    args.get(index + 1)
                        .ok_or("`--backend` needs a value.")?
                        .clone(),
                );
                index += 2;
            }
            "--out" => {
                out = Some(PathBuf::from(
                    args.get(index + 1).ok_or("`--out` needs a value.")?,
                ));
                index += 2;
            }
            positional => {
                if input.is_some() {
                    return Err(format!("Unexpected argument `{positional}`."));
                }
                input = Some(PathBuf::from(positional));
                index += 1;
            }
        }
    }
    Ok(LowerArgs {
        backend: backend.ok_or("`ecky lower` needs `--backend`.")?,
        input: input.ok_or("`ecky lower` needs an input file.")?,
        out: out.ok_or("`ecky lower` needs `--out`.")?,
    })
}

fn run_lower(args: &[String]) -> ExitCode {
    let parsed = match parse_lower_args(args) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("{message}");
            print_usage();
            return ExitCode::from(2);
        }
    };
    let source = match std::fs::read_to_string(&parsed.input) {
        Ok(source) => source,
        Err(err) => {
            eprintln!("Failed to read `{}`: {err}", parsed.input.display());
            return ExitCode::from(1);
        }
    };
    let lower: fn(&str) -> ecky_cad_lib::models::AppResult<String> = match parsed.backend.as_str() {
        "build123d" => ecky_cad_lib::ecky_ir::lower_to_build123d,
        "freecad" => ecky_cad_lib::ecky_ir::lower_to_freecad,
        other => {
            eprintln!("Unsupported backend `{other}`. Use `build123d` or `freecad`.");
            return ExitCode::from(2);
        }
    };
    // Lowering recurses deeply on large models; match the app's guarded stack.
    let lowered = std::thread::Builder::new()
        .stack_size(LOWER_STACK_SIZE)
        .spawn(move || lower(&source))
        .expect("spawn lowering thread")
        .join();
    match lowered {
        Ok(Ok(lowered)) => {
            if let Some(parent) = parsed.out.parent() {
                if let Err(err) = std::fs::create_dir_all(parent) {
                    eprintln!("Failed to create `{}`: {err}", parent.display());
                    return ExitCode::from(1);
                }
            }
            if let Err(err) = std::fs::write(&parsed.out, lowered) {
                eprintln!("Failed to write `{}`: {err}", parsed.out.display());
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        Ok(Err(err)) => {
            eprintln!("{err}");
            ExitCode::from(1)
        }
        Err(_) => {
            eprintln!("Lowering panicked.");
            ExitCode::from(1)
        }
    }
}
