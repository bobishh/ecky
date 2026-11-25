//! CAD → Ecky transpile harness.
//!
//! Assembles the shared Ecky language reference as the system prompt plus a fixed
//! translate instruction (see `cad_transpile`), sends a foreign CAD source as the
//! user message through the existing OpenAI-compatible client, and prints the
//! returned `.ecky`. Provider/model/key/base-url resolve from the app config
//! first, env + flags override. `--dump-prompt` prints the assembled prompt with
//! no network call (free inspection / cross-model diffing).

use std::fs;
use std::path::PathBuf;

use ecky_cad_lib::cad_source_adapters::{adapt_cad_source, SystemCadSourceCommandRunner};
use ecky_cad_lib::cad_transpile::{build_transpile_messages, strip_code_fence};
use ecky_cad_lib::contracts::GeometryBackend;
use ecky_cad_lib::llm::{extract_openai_message_content, send_openai_request};
use ecky_cad_lib::steel_data::parse_steel_data;

fn usage() -> &'static str {
    "Usage: cad_to_ecky <input> [--backend mesh|freecad] [--model M] \
[--base-url URL] [--api-key K] [--config config.edn] [--out out.ecky] [--dump-prompt]"
}

fn parse_backend(s: &str) -> Result<GeometryBackend, String> {
    serde_json::from_value(serde_json::Value::String(s.to_string()))
        .map_err(|_| format!("unknown backend '{s}' (use mesh|freecad)"))
}

/// Route source formats through text-only adapters. No adapter emits Ecky.
fn read_transpile_source(input: &std::path::Path) -> Result<String, String> {
    let bytes =
        fs::read(input).map_err(|error| format!("read input '{}': {error}", input.display()))?;
    adapt_cad_source(input, &bytes, &SystemCadSourceCommandRunner)
}

/// Resolve the app config path: explicit flag, then `ECKY_APP_CONFIG_DIR`, then
/// the same platform location used by Tauri's app config directory.
fn config_path(explicit: Option<PathBuf>) -> Result<(PathBuf, bool), String> {
    if let Some(path) = explicit {
        validate_config_path(&path)?;
        return Ok((path, true));
    }
    if let Some(dir) = std::env::var_os("ECKY_APP_CONFIG_DIR") {
        return Ok((PathBuf::from(dir).join("config.edn"), false));
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME").ok_or("HOME not set; pass --config")?;
        Ok((
            PathBuf::from(home)
                .join("Library/Application Support/com.alcoholics-audacious.ecky-cad/config.edn"),
            false,
        ))
    }
    #[cfg(target_os = "windows")]
    {
        let app_data = std::env::var_os("APPDATA").ok_or("APPDATA not set; pass --config")?;
        return Ok((
            PathBuf::from(app_data).join("com.alcoholics-audacious.ecky-cad/config.edn"),
            false,
        ));
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .ok_or("XDG_CONFIG_HOME and HOME not set; pass --config")?;
        Ok((
            base.join("com.alcoholics-audacious.ecky-cad/config.edn"),
            false,
        ))
    }
}

fn validate_config_path(path: &std::path::Path) -> Result<(), String> {
    if path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
    {
        return Err("--config accepts canonical EDN only".to_string());
    }
    Ok(())
}

struct Resolved {
    base_url: String,
    api_key: String,
    model: String,
    backend: GeometryBackend,
}

fn resolve(
    cfg_path: &std::path::Path,
    explicit_config: bool,
    flag_model: Option<String>,
    flag_base: Option<String>,
    flag_key: Option<String>,
    flag_backend: Option<GeometryBackend>,
) -> Result<Resolved, String> {
    let config = match fs::read_to_string(cfg_path) {
        Ok(data) => {
            let data =
                parse_steel_data(&data).map_err(|_| "config.edn: invalid-data".to_string())?;
            Some(
                ecky_cad_lib::contracts::decode_config(&data)
                    .map_err(|_| "config.edn: invalid-shape".to_string())?,
            )
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !explicit_config => None,
        Err(_) => return Err("config.edn: read-failed".to_string()),
    };

    let engine = config
        .as_ref()
        .map(|config| {
            config
                .engines
                .iter()
                .find(|engine| engine.id == config.selected_engine_id)
                .ok_or_else(|| "config.edn: selected-engine-missing".to_string())
        })
        .transpose()?;

    let pick = |flag: Option<String>,
                nvidia_env: &str,
                nim_env: &str,
                config_value: Option<&str>,
                fallback: &str|
     -> String {
        flag.or_else(|| std::env::var(nvidia_env).ok())
            .or_else(|| std::env::var(nim_env).ok())
            .or_else(|| config_value.map(str::to_owned))
            .unwrap_or_else(|| fallback.to_owned())
    };

    Ok(Resolved {
        base_url: pick(
            flag_base,
            "NVIDIA_BASE_URL",
            "NIM_BASE_URL",
            engine.map(|engine| engine.base_url.as_str()),
            "https://integrate.api.nvidia.com/v1",
        ),
        api_key: pick(
            flag_key,
            "NVIDIA_API_KEY",
            "NIM_API_KEY",
            engine.map(|engine| engine.api_key.as_str()),
            "",
        ),
        model: pick(
            flag_model,
            "NVIDIA_MODEL",
            "NIM_MODEL",
            engine.map(|engine| engine.model.as_str()),
            "",
        ),
        backend: flag_backend.unwrap_or_else(|| {
            config
                .map(|config| config.default_geometry_backend)
                .unwrap_or_default()
        }),
    })
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut input: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut cfg: Option<PathBuf> = None;
    let (mut model, mut base, mut key) = (None, None, None);
    let mut backend: Option<GeometryBackend> = None;
    let mut dump_prompt = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" | "-o" => out = Some(PathBuf::from(args.next().ok_or(usage())?)),
            "--config" => cfg = Some(PathBuf::from(args.next().ok_or(usage())?)),
            "--model" => model = Some(args.next().ok_or(usage())?),
            "--base-url" => base = Some(args.next().ok_or(usage())?),
            "--api-key" => key = Some(args.next().ok_or(usage())?),
            "--backend" => backend = Some(parse_backend(&args.next().ok_or(usage())?)?),
            "--dump-prompt" => dump_prompt = true,
            "--help" | "-h" => {
                println!("{}", usage());
                return Ok(());
            }
            _ if input.is_none() => input = Some(PathBuf::from(arg)),
            _ => return Err(usage().to_string()),
        }
    }

    let input = input.ok_or(usage())?;
    if let Some(path) = cfg.as_deref() {
        validate_config_path(path)?;
    }
    let source = read_transpile_source(&input)?;

    // --dump-prompt is network-free: it only needs the backend.
    if dump_prompt {
        let chosen = backend.unwrap_or_default();
        let (system, user) = build_transpile_messages(&source, chosen);
        println!("===== SYSTEM ({chosen:?}) =====\n{system}\n\n===== USER =====\n{user}");
        return Ok(());
    }

    let cfg_path = config_path(cfg)?;
    let r = resolve(&cfg_path.0, cfg_path.1, model, base, key, backend)?;
    if r.api_key.is_empty() {
        return Err("no API key (config/env/--api-key all empty)".to_string());
    }

    let (system, user) = build_transpile_messages(&source, r.backend);
    let payload = serde_json::json!({
        "model": r.model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
        "temperature": 0.2,
    });
    let url = format!("{}/chat/completions", r.base_url.trim_end_matches('/'));
    let client = reqwest::Client::new();

    // NIM hosted models can return a transient 500 "instance not found" while a
    // cold instance spins up; retry a few times before giving up.
    let mut last_err = String::new();
    let mut ecky = None;
    for attempt in 1..=6 {
        match send_openai_request(&client, &url, &r.api_key, &payload).await {
            Ok((status, body)) if status.is_success() => {
                let json: serde_json::Value =
                    serde_json::from_str(&body).map_err(|e| format!("parse response: {e}"))?;
                ecky = Some(strip_code_fence(&extract_openai_message_content(&json)?));
                break;
            }
            Ok((status, body)) => {
                last_err = format!("HTTP {status}: provider-request-failed");
                let cold = status.as_u16() == 500 || status.as_u16() == 503;
                if cold && body.contains("not found") && attempt < 6 {
                    eprintln!("attempt {attempt}: cold instance, retrying…");
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    continue;
                }
                break;
            }
            Err(_) => {
                last_err = "provider-transport-failed".to_string();
                break;
            }
        }
    }
    let ecky = ecky.ok_or_else(|| format!("transpile failed: {last_err}"))?;

    eprintln!("model={} backend={:?}", r.model, r.backend);
    if let Some(out) = out {
        fs::write(&out, &ecky).map_err(|e| format!("write '{}': {e}", out.display()))?;
    } else {
        println!("{ecky}");
    }
    Ok(())
}
