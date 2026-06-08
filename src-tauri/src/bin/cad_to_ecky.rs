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

use ecky_cad_lib::cad_transpile::{build_transpile_messages, strip_code_fence};
use ecky_cad_lib::contracts::Config;
use ecky_cad_lib::llm::{extract_openai_message_content, send_openai_request};
use ecky_cad_lib::models::GeometryBackend;

fn usage() -> &'static str {
    "Usage: cad_to_ecky <input> [--backend mesh|build123d|freecad] [--model M] \
[--base-url URL] [--api-key K] [--config config.json] [--out out.ecky] [--dump-prompt]"
}

fn parse_backend(s: &str) -> Result<GeometryBackend, String> {
    serde_json::from_value(serde_json::Value::String(s.to_string()))
        .map_err(|_| format!("unknown backend '{s}' (use mesh|build123d|freecad)"))
}

/// Resolve the app config path: explicit flag, then `ECKY_APP_CONFIG_DIR`, then
/// the platform default under the user's config dir.
fn config_path(explicit: Option<PathBuf>) -> Result<PathBuf, String> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    if let Some(dir) = std::env::var_os("ECKY_APP_CONFIG_DIR") {
        return Ok(PathBuf::from(dir).join("config.json"));
    }
    let home = std::env::var_os("HOME").ok_or("HOME not set; pass --config")?;
    Ok(PathBuf::from(home)
        .join("Library/Application Support/com.alcoholics-audacious.ecky-cad/config.json"))
}

struct Resolved {
    base_url: String,
    api_key: String,
    model: String,
    backend: GeometryBackend,
}

fn resolve(
    cfg_path: &PathBuf,
    flag_model: Option<String>,
    flag_base: Option<String>,
    flag_key: Option<String>,
    flag_backend: Option<GeometryBackend>,
) -> Result<Resolved, String> {
    let data = fs::read_to_string(cfg_path)
        .map_err(|e| format!("read config '{}': {e}", cfg_path.display()))?;
    let config: Config =
        serde_json::from_str(&data).map_err(|e| format!("parse config: {e}"))?;
    let engine = config
        .engines
        .iter()
        .find(|e| e.id == config.selected_engine_id)
        .ok_or("no selected engine in config")?;

    let pick = |flag: Option<String>, env: &str, base: &str| -> String {
        flag.or_else(|| std::env::var(env).ok())
            .unwrap_or_else(|| base.to_string())
    };

    Ok(Resolved {
        base_url: pick(flag_base, "NVIDIA_BASE_URL", &engine.base_url),
        api_key: pick(flag_key, "NVIDIA_API_KEY", &engine.api_key),
        model: pick(flag_model, "NVIDIA_MODEL", &engine.model),
        backend: flag_backend.unwrap_or(config.default_geometry_backend),
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
    let source = fs::read_to_string(&input)
        .map_err(|e| format!("read input '{}': {e}", input.display()))?;

    // --dump-prompt is network-free: it only needs the backend.
    if dump_prompt {
        let chosen = backend.unwrap_or_default();
        let (system, user) = build_transpile_messages(&source, chosen);
        println!("===== SYSTEM ({chosen:?}) =====\n{system}\n\n===== USER =====\n{user}");
        return Ok(());
    }

    let cfg_path = config_path(cfg)?;
    let r = resolve(&cfg_path, model, base, key, backend)?;
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
                let json: serde_json::Value = serde_json::from_str(&body)
                    .map_err(|e| format!("parse response: {e}"))?;
                ecky = Some(strip_code_fence(&extract_openai_message_content(&json)?));
                break;
            }
            Ok((status, body)) => {
                last_err = format!("HTTP {status}: {}", body.chars().take(200).collect::<String>());
                let cold = status.as_u16() == 500 || status.as_u16() == 503;
                if cold && body.contains("not found") && attempt < 6 {
                    eprintln!("attempt {attempt}: cold instance, retrying…");
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    continue;
                }
                break;
            }
            Err(e) => {
                last_err = e;
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
