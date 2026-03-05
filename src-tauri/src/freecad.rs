use std::process::Command;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};
use sha2::{Sha256, Digest};

pub fn render(macro_code: &str, parameters: &serde_json::Value, app: &AppHandle) -> Result<String, String> {
    let app_dir = app.path().app_data_dir().unwrap_or_else(|_| PathBuf::from("."));
    fs::create_dir_all(&app_dir).map_err(|e| e.to_string())?;
    
    let params_json = serde_json::to_string(parameters).map_err(|e| e.to_string())?;
    
    // Create a deterministic digest based on macro code and parameters
    let mut hasher = Sha256::new();
    hasher.update(macro_code.as_bytes());
    hasher.update(b"|"); // separator
    hasher.update(params_json.as_bytes());
    let result = hasher.finalize();
    let digest_str = format!("{:x}", result);

    // Limit length to avoid path length issues, 32 chars is plenty for collision resistance here
    let short_digest = &digest_str[..32]; 

    let macro_path = app_dir.join(format!("{}.FCMacro", short_digest));
    let stl_path = app_dir.join(format!("{}.stl", short_digest));
    
    // CACHE HIT: If the STL already exists for this exact code + parameters, return it instantly.
    if stl_path.exists() {
        return Ok(stl_path.to_str().ok_or("Invalid result path")?.to_string());
    }

    fs::write(&macro_path, macro_code).map_err(|e| e.to_string())?;
    
    let freecad_cmd = resolve_freecad_path();
    let runner_path = resolve_runner_path(app);

    let output = Command::new(&freecad_cmd)
        .arg(runner_path.to_str().ok_or("Invalid runner path")?)
        .env("DRYDEMACHER_MACRO", macro_path.to_str().ok_or("Invalid macro path")?)
        .env("DRYDEMACHER_STL", stl_path.to_str().ok_or("Invalid stl path")?)
        .env("DRYDEMACHER_PARAMS", &params_json)
        .output()
        .map_err(|e| format!("Failed to execute FreeCAD ({}): {}", freecad_cmd, e))?;

    if !output.status.success() {
        return Err(format!("FreeCAD Error: {}", String::from_utf8_lossy(&output.stderr)));
    }

    Ok(stl_path.to_str().ok_or("Invalid result path")?.to_string())
}

pub fn get_default_macro(app: &AppHandle) -> Result<String, String> {
    let mut path = PathBuf::from("../templates/cache_pot_default.FCMacro");
    if !path.exists() {
        let resource_path = app.path().resource_dir().unwrap_or_default();
        path = resource_path.join("templates/cache_pot_default.FCMacro");
    }

    if !path.exists() {
        path = PathBuf::from("templates/cache_pot_default.FCMacro");
    }

    fs::read_to_string(path).map_err(|e| format!("Failed to read default macro: {}", e))
}

fn resolve_freecad_path() -> String {
    let env_cmd = std::env::var("FREECAD_CMD").unwrap_or_else(|_| "FreeCADCmd".to_string());
    if env_cmd == "FreeCADCmd" && !Path::new(&env_cmd).exists() {
        let mac_path = "/Applications/FreeCAD.app/Contents/Resources/bin/freecadcmd";
        if Path::new(mac_path).exists() {
            return mac_path.to_string();
        }
    }
    env_cmd
}

fn resolve_runner_path(app: &AppHandle) -> PathBuf {
    let resource_path = app.path().resource_dir().unwrap_or_default();
    let mut runner_path = resource_path.join("server/freecad_runner.py");
    if !runner_path.exists() {
        runner_path = PathBuf::from("../server/freecad_runner.py");
    }
    runner_path
}
