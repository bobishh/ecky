use std::path::PathBuf;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Deserialize;
use tauri::{AppHandle, State};
use tokio::process::Command;
use uuid::Uuid;

use crate::contracts::{PromptTranscription, TranscribePromptAudioInput};
use crate::models::{AppError, AppErrorCode, AppResult, AppState, Config, Engine, PathResolver};

const NVIDIA_SPEECH_PROVIDER: &str = "nvidia-speech";
const DEFAULT_NVIDIA_SPEECH_SERVER: &str = "grpc.nvcf.nvidia.com:443";
const DEFAULT_NVIDIA_SPEECH_MODEL: &str = "parakeet-tdt-0.6b-v2";
const DEFAULT_NVIDIA_SPEECH_FUNCTION_ID: &str = "d3fe9151-442b-4204-a70d-5fcc597fd610";
const DEFAULT_NVIDIA_SPEECH_LANGUAGE: &str = "en-US";
const SPEECH_PYTHON_RESOURCE_CANDIDATES: &[&str] =
    &["runtime/speech/bin/python3", "runtime/speech/bin/python"];
const SPEECH_PYTHON_FALLBACK_CANDIDATES: &[&str] = &[
    ".dist/speech-runtime/bin/python3",
    ".dist/speech-runtime/bin/python",
];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PythonSpeechResult {
    text: Option<String>,
    error: Option<String>,
    details: Option<String>,
    code: Option<String>,
}

#[derive(Debug, Clone)]
struct NvidiaSpeechEngine {
    api_key: String,
}

#[derive(Debug, Clone)]
struct NvidiaSpeechRuntime {
    server: String,
    function_id: String,
    model: String,
    language_code: String,
    python_cmd: PathBuf,
}

impl NvidiaSpeechRuntime {
    fn from_input(input: &TranscribePromptAudioInput, app: &dyn PathResolver) -> AppResult<Self> {
        Ok(Self {
            server: std::env::var("ECKY_NVIDIA_SPEECH_SERVER")
                .unwrap_or_else(|_| DEFAULT_NVIDIA_SPEECH_SERVER.to_string()),
            function_id: std::env::var("ECKY_NVIDIA_SPEECH_FUNCTION_ID")
                .unwrap_or_else(|_| DEFAULT_NVIDIA_SPEECH_FUNCTION_ID.to_string()),
            model: std::env::var("ECKY_NVIDIA_SPEECH_MODEL")
                .unwrap_or_else(|_| DEFAULT_NVIDIA_SPEECH_MODEL.to_string()),
            language_code: input
                .language_code
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .or_else(|| std::env::var("ECKY_NVIDIA_SPEECH_LANGUAGE").ok())
                .unwrap_or_else(|| DEFAULT_NVIDIA_SPEECH_LANGUAGE.to_string()),
            python_cmd: resolve_speech_python_cmd(app)?,
        })
    }
}

fn resolve_speech_python_cmd(app: &dyn PathResolver) -> AppResult<PathBuf> {
    if let Ok(cmd) = std::env::var("ECKY_NVIDIA_SPEECH_PYTHON") {
        let trimmed = cmd.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }

    for resource in SPEECH_PYTHON_RESOURCE_CANDIDATES {
        if let Some(path) = app.resource_path(resource) {
            if path.exists() {
                return Ok(path);
            }
        }
    }

    for fallback in SPEECH_PYTHON_FALLBACK_CANDIDATES {
        let path = PathBuf::from(fallback);
        if path.exists() {
            return Ok(path);
        }
    }

    Err(AppError::provider(
        "Bundled NVIDIA Speech runtime is missing. Reinstall Ecky or run runtime repair.",
    ))
}

fn is_nvidia_engine(engine: &Engine) -> bool {
    let haystack = format!(
        "{} {} {} {}",
        engine.provider, engine.base_url, engine.model, engine.name
    )
    .to_lowercase();
    haystack.contains("nvidia")
        || haystack.contains("integrate.api.nvidia.com")
        || haystack.contains("nim")
}

fn selected_nvidia_speech_engine(config: &Config) -> AppResult<NvidiaSpeechEngine> {
    let selected = config
        .engines
        .iter()
        .find(|engine| engine.id == config.selected_engine_id);
    let engine = selected
        .filter(|engine| {
            engine.enabled && is_nvidia_engine(engine) && !engine.api_key.trim().is_empty()
        })
        .or_else(|| {
            config.engines.iter().find(|engine| {
                engine.enabled && is_nvidia_engine(engine) && !engine.api_key.trim().is_empty()
            })
        })
        .ok_or_else(|| {
            AppError::provider(
                "NVIDIA Speech transcription needs an enabled NVIDIA NIM engine with an API key.",
            )
        })?;

    Ok(NvidiaSpeechEngine {
        api_key: engine.api_key.clone(),
    })
}

fn validate_audio_input(input: &TranscribePromptAudioInput) -> AppResult<()> {
    if input.base64_data.trim().is_empty() {
        return Err(AppError::validation("Voice input was empty."));
    }
    if !input.mime_type.starts_with("audio/") {
        return Err(AppError::validation(format!(
            "Voice input must be audio, got {}.",
            input.mime_type
        )));
    }
    Ok(())
}

fn temp_audio_path(mime_type: &str) -> PathBuf {
    let ext = if mime_type.contains("wav") {
        "wav"
    } else if mime_type.contains("ogg") {
        "ogg"
    } else if mime_type.contains("opus") {
        "opus"
    } else {
        "audio"
    };
    std::env::temp_dir().join(format!("ecky-voice-{}.{}", Uuid::new_v4(), ext))
}

fn python_adapter_source() -> &'static str {
    r#"
import json
import os
import sys

audio_path = sys.argv[1]
server = sys.argv[2]
function_id = sys.argv[3]
language_code = sys.argv[4]
api_key = os.environ.get("NVIDIA_API_KEY", "")

try:
    import grpc
    import riva.client
except Exception as exc:
    print(json.dumps({
        "error": "Bundled NVIDIA Speech runtime is missing the Riva client.",
        "details": f"Reinstall Ecky or rebuild the bundled speech runtime. Import error: {type(exc).__name__}: {exc}",
    }), file=sys.stderr)
    sys.exit(2)

try:
    auth = riva.client.Auth(
        use_ssl=True,
        uri=server,
        metadata_args=[
            ["function-id", function_id],
            ["authorization", f"Bearer {api_key}"],
        ],
        options=[
            ("grpc.max_receive_message_length", 64 * 1024 * 1024),
            ("grpc.max_send_message_length", 64 * 1024 * 1024),
        ],
    )
    asr_service = riva.client.ASRService(auth)
    config = riva.client.RecognitionConfig(
        language_code=language_code,
        max_alternatives=1,
        enable_automatic_punctuation=True,
    )
    with open(audio_path, "rb") as fh:
        data = fh.read()
    response = asr_service.offline_recognize(data, config)
    text = " ".join(
        result.alternatives[0].transcript.strip()
        for result in response.results
        if result.alternatives and result.alternatives[0].transcript.strip()
    ).strip()
    print(json.dumps({"text": text}))
except grpc.RpcError as exc:
    print(json.dumps({
        "error": "NVIDIA Speech provider error.",
        "details": exc.details() or str(exc),
        "code": str(exc.code()),
    }), file=sys.stderr)
    sys.exit(3)
except Exception as exc:
    print(json.dumps({
        "error": "NVIDIA Speech adapter error.",
        "details": f"{type(exc).__name__}: {exc}",
    }), file=sys.stderr)
    sys.exit(4)
"#
}

fn parse_python_failure(stderr: &[u8], stdout: &[u8]) -> AppError {
    let raw_stderr = String::from_utf8_lossy(stderr).trim().to_string();
    let raw_stdout = String::from_utf8_lossy(stdout).trim().to_string();
    let raw = if raw_stderr.is_empty() {
        raw_stdout
    } else {
        raw_stderr
    };

    if let Ok(parsed) = serde_json::from_str::<PythonSpeechResult>(&raw) {
        let message = parsed
            .error
            .unwrap_or_else(|| "NVIDIA Speech transcription failed.".to_string());
        let details = [parsed.code, parsed.details]
            .into_iter()
            .flatten()
            .filter(|part| !part.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if details.is_empty() {
            AppError::provider(message)
        } else {
            AppError::with_details(AppErrorCode::Provider, message, details)
        }
    } else if raw.is_empty() {
        AppError::provider("NVIDIA Speech transcription failed without provider detail.")
    } else {
        AppError::with_details(
            AppErrorCode::Provider,
            "NVIDIA Speech transcription failed.",
            raw,
        )
    }
}

async fn transcribe_with_nvidia_speech(
    engine: NvidiaSpeechEngine,
    runtime: NvidiaSpeechRuntime,
    audio_path: PathBuf,
) -> AppResult<PromptTranscription> {
    let output = Command::new(&runtime.python_cmd)
        .arg("-c")
        .arg(python_adapter_source())
        .arg(&audio_path)
        .arg(&runtime.server)
        .arg(&runtime.function_id)
        .arg(&runtime.language_code)
        .env("NVIDIA_API_KEY", engine.api_key)
        .output()
        .await
        .map_err(|err| {
            AppError::with_details(
                AppErrorCode::Provider,
                format!(
                    "Failed to start NVIDIA Speech adapter with {}.",
                    runtime.python_cmd.display()
                ),
                err.to_string(),
            )
        })?;

    if !output.status.success() {
        return Err(parse_python_failure(&output.stderr, &output.stdout));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed = serde_json::from_str::<PythonSpeechResult>(stdout.trim()).map_err(|err| {
        AppError::with_details(
            AppErrorCode::Provider,
            "NVIDIA Speech adapter returned invalid JSON.",
            format!("{}\n{}", err, stdout.trim()),
        )
    })?;
    let text = parsed.text.unwrap_or_default().trim().to_string();
    if text.is_empty() {
        return Err(AppError::provider(
            "NVIDIA Speech returned an empty transcript.",
        ));
    }

    Ok(PromptTranscription {
        text,
        provider: NVIDIA_SPEECH_PROVIDER.to_string(),
        model: runtime.model,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn transcribe_prompt_audio(
    input: TranscribePromptAudioInput,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<PromptTranscription> {
    validate_audio_input(&input)?;
    let engine = {
        let config = state.config.lock().unwrap();
        selected_nvidia_speech_engine(&config)?
    };
    let runtime = NvidiaSpeechRuntime::from_input(&input, &app)?;
    let audio_path = temp_audio_path(&input.mime_type);
    let audio = STANDARD
        .decode(input.base64_data.as_bytes())
        .map_err(|err| {
            AppError::validation(format!("Voice audio base64 decode failed: {}", err))
        })?;

    tokio::fs::write(&audio_path, audio)
        .await
        .map_err(|err| AppError::persistence(err.to_string()))?;
    let result = transcribe_with_nvidia_speech(engine, runtime, audio_path.clone()).await;
    let _ = tokio::fs::remove_file(audio_path).await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Config, EngineKind, GeometryBackend, McpConfig, SourceLanguage};

    fn engine(id: &str, name: &str, base_url: &str, api_key: &str, enabled: bool) -> Engine {
        Engine {
            id: id.to_string(),
            name: name.to_string(),
            provider: "openai".to_string(),
            api_key: api_key.to_string(),
            model: "deepseek-ai/deepseek-v4-pro".to_string(),
            light_model: String::new(),
            base_url: base_url.to_string(),
            system_prompt: String::new(),
            enabled,
        }
    }

    fn config(engines: Vec<Engine>, selected_engine_id: &str) -> Config {
        Config {
            engines,
            selected_engine_id: selected_engine_id.to_string(),
            freecad_cmd: String::new(),
            assets: vec![],
            microwave: None,
            voice: crate::models::VoiceConfig::default(),
            mcp: McpConfig::default(),
            has_seen_onboarding: true,
            connection_type: None,
            default_engine_kind: EngineKind::Freecad,
            default_source_language: SourceLanguage::LegacyPython,
            default_geometry_backend: GeometryBackend::Freecad,
            max_generation_attempts: 3,
            max_verify_attempts: 0,
        }
    }

    #[test]
    fn selected_nvidia_speech_engine_uses_selected_nim_key() {
        let cfg = config(
            vec![
                engine(
                    "openai",
                    "OpenAI",
                    "https://api.openai.com/v1",
                    "openai-key",
                    true,
                ),
                engine(
                    "nim",
                    "NVIDIA NIM",
                    "https://integrate.api.nvidia.com/v1",
                    "nim-key",
                    true,
                ),
            ],
            "nim",
        );

        let selected = selected_nvidia_speech_engine(&cfg).expect("engine");

        assert_eq!(selected.api_key, "nim-key");
    }

    #[test]
    fn selected_nvidia_speech_engine_falls_back_to_enabled_nim_key() {
        let cfg = config(
            vec![
                engine(
                    "openai",
                    "OpenAI",
                    "https://api.openai.com/v1",
                    "openai-key",
                    true,
                ),
                engine(
                    "nim",
                    "NVIDIA NIM",
                    "https://integrate.api.nvidia.com/v1",
                    "nim-key",
                    true,
                ),
            ],
            "openai",
        );

        let selected = selected_nvidia_speech_engine(&cfg).expect("engine");

        assert_eq!(selected.api_key, "nim-key");
    }

    #[test]
    fn selected_nvidia_speech_engine_requires_enabled_nim_key() {
        let cfg = config(
            vec![engine(
                "nim",
                "NVIDIA NIM",
                "https://integrate.api.nvidia.com/v1",
                "",
                true,
            )],
            "nim",
        );

        let err = selected_nvidia_speech_engine(&cfg).expect_err("missing key");

        assert!(err.message.contains("NVIDIA NIM engine"));
    }

    #[test]
    fn parse_python_failure_preserves_provider_detail() {
        let err = parse_python_failure(
            br#"{"error":"NVIDIA Speech provider error.","details":"401 Unauthorized: invalid API key","code":"StatusCode.UNAUTHENTICATED"}"#,
            b"",
        );

        assert!(err.message.contains("provider error"));
        assert!(err.details.unwrap().contains("401 Unauthorized"));
    }

    #[test]
    fn python_adapter_error_points_to_bundled_runtime_not_user_pip() {
        let source = python_adapter_source();

        assert!(source.contains("bundled speech runtime"));
        assert!(!source.contains("pip install"));
    }
}
