use crate::freecad::resolve_resource_path;
use crate::mcp::contracts::{CompareModelsRequest, CompareModelsResponse};
use crate::models::{AppError, AppErrorCode, AppResult, PathResolver};

pub async fn handle_compare_models(
    app: &dyn PathResolver,
    req: CompareModelsRequest,
) -> AppResult<CompareModelsResponse> {
    let script_path = resolve_resource_path(
        app,
        "server/compare_metric.py",
        &["../server/compare_metric.py", "server/compare_metric.py"],
    )?;

    let output = std::process::Command::new("python3")
        .arg(script_path)
        .arg(&req.ref_path)
        .arg(&req.gen_path)
        .output()
        .map_err(|e| {
            AppError::new(
                AppErrorCode::Internal,
                format!("Failed to execute comparison script: {}", e),
            )
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        return Err(AppError::new(
            AppErrorCode::Internal,
            format!("Comparison script failed: {}\n{}", stdout, stderr),
        ));
    }

    let mut ref_vol = 0.0;
    let mut gen_vol = 0.0;
    let mut vol_diff = 0.0;
    let mut bb_err = 0.0;
    let mut status = "UNKNOWN".to_string();

    for line in stdout.lines() {
        if line.starts_with("Reference Volume:") {
            ref_vol = parse_metric(line);
        } else if line.starts_with("Generated Volume:") {
            gen_vol = parse_metric(line);
        } else if line.starts_with("Volume Difference:") {
            vol_diff = parse_metric(line);
        } else if line.starts_with("Bounding Box Match Error:") {
            bb_err = parse_metric(line);
        } else if line.starts_with("Status:") {
            status = line
                .strip_prefix("Status: ")
                .unwrap_or(line)
                .trim()
                .to_string();
        }
    }

    Ok(CompareModelsResponse {
        reference_volume: ref_vol,
        generated_volume: gen_vol,
        volume_difference_percent: vol_diff,
        bounding_box_match_error: bb_err,
        status,
        details: stdout.into_owned(),
    })
}

fn parse_metric(line: &str) -> f64 {
    line.split(':')
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0)
}
