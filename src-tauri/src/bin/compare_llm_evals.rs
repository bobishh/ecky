use ecky_cad_lib::llm_eval::{compare_runs, read_run};
use std::path::PathBuf;

fn run() -> Result<(), String> {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if !(2..=3).contains(&arguments.len()) {
        return Err(
            "usage: compare_llm_evals <baseline-run-dir> <challenger-run-dir> [report.md]".into(),
        );
    }
    let baseline = read_run(&PathBuf::from(&arguments[0]))?;
    let challenger = read_run(&PathBuf::from(&arguments[1]))?;
    let report = compare_runs(&baseline, &challenger)?;
    if let Some(output) = arguments.get(2) {
        let output = PathBuf::from(output);
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(output, report).map_err(|error| error.to_string())?;
    } else {
        print!("{report}");
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
