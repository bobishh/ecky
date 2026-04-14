use std::fs;
use std::path::PathBuf;

fn usage() -> &'static str {
    "Usage: lower_ecky_ir_to_build123d <input.ecky> [--out <output.py>]"
}

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut input_path: Option<PathBuf> = None;
    let mut output_path: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" | "-o" => {
                let Some(path) = args.next() else {
                    return Err(usage().to_string());
                };
                output_path = Some(PathBuf::from(path));
            }
            "--help" | "-h" => {
                println!("{}", usage());
                return Ok(());
            }
            _ if input_path.is_none() => input_path = Some(PathBuf::from(arg)),
            _ => return Err(usage().to_string()),
        }
    }

    let Some(input_path) = input_path else {
        return Err(usage().to_string());
    };

    let source = fs::read_to_string(&input_path)
        .map_err(|err| format!("Failed to read '{}': {}", input_path.display(), err))?;
    let lowered =
        ecky_cad_lib::ecky_ir::lower_to_build123d(&source).map_err(|err| err.to_string())?;

    if let Some(output_path) = output_path {
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "Failed to create output dir '{}': {}",
                    parent.display(),
                    err
                )
            })?;
        }
        fs::write(&output_path, lowered)
            .map_err(|err| format!("Failed to write '{}': {}", output_path.display(), err))?;
    } else {
        print!("{}", lowered);
    }

    Ok(())
}
