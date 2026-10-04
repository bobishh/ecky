//! Typed, canonical Config v1 EDN boundary. Diagnostics deliberately name fields only.

use super::config::VisionCapability;
use super::{
    AppError, AppResult, Asset, AutoAgent, Config, Engine, EngineKind, FemComputeConfig,
    FemComputeQuality, GeometryBackend, JevClassifierConfig, McpConfig, McpMode, MicrowaveConfig,
    ProviderModels, SourceLanguage, VoiceConfig,
};
use crate::steel_data::{validate_steel_data, SteelDataValue};
use std::collections::{HashMap, HashSet};
use std::path::Path;

const SCHEMA: &str = "ecky/config";
pub const CONFIG_DEPRECATED_START_ON_DEMAND_DROPPED: &str =
    "CONFIG_DEPRECATED_START_ON_DEMAND_DROPPED";
pub const CONFIG_NONCANONICAL_DEPRECATED_FIELD: &str = "CONFIG_NONCANONICAL_DEPRECATED_FIELD";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigNormalizationWarning {
    pub code: &'static str,
    pub field: &'static str,
}

pub fn normalize_legacy_config_for_edn(config: &mut Config) -> Vec<ConfigNormalizationWarning> {
    let mut warnings = Vec::new();
    for agent in &mut config.mcp.auto_agents {
        if agent.start_on_demand {
            agent.start_on_demand = false;
            warnings.push(ConfigNormalizationWarning {
                code: CONFIG_DEPRECATED_START_ON_DEMAND_DROPPED,
                field: "mcp.autoAgents[].startOnDemand",
            });
        }
    }
    warnings
}

fn err(field: &str, reason: &str) -> AppError {
    AppError::parse(format!("invalid config field {field}: {reason}"))
}
fn map(entries: Vec<(&str, SteelDataValue)>) -> SteelDataValue {
    SteelDataValue::Map(
        entries
            .into_iter()
            .map(|(k, v)| (format!(":{k}"), v))
            .collect(),
    )
}
fn key(s: &str) -> SteelDataValue {
    SteelDataValue::Keyword(format!(":{s}"))
}
fn opt_string(value: &Option<String>) -> SteelDataValue {
    value
        .clone()
        .map(SteelDataValue::String)
        .unwrap_or(SteelDataValue::Nil)
}
fn opt_int<T: Into<i64> + Copy>(value: Option<T>) -> SteelDataValue {
    value
        .map(|v| SteelDataValue::Integer(v.into()))
        .unwrap_or(SteelDataValue::Nil)
}

pub fn encode_config(config: &Config) -> AppResult<SteelDataValue> {
    config.jev_classifier.validate()?;
    unique_ids(config.engines.iter().map(|x| x.id.as_str()), "engines")?;
    unique_ids(config.assets.iter().map(|x| x.id.as_str()), "assets")?;
    unique_ids(
        config.mcp.auto_agents.iter().map(|x| x.id.as_str()),
        "auto-agents",
    )?;
    unique_ids(
        config.mcp.auto_agents.iter().map(|x| x.label.as_str()),
        "auto-agents.labels",
    )?;
    if config.mcp.auto_agents.iter().any(|x| x.start_on_demand) {
        return Err(err("auto-agents", CONFIG_NONCANONICAL_DEPRECATED_FIELD));
    }
    for asset in &config.assets {
        validate_asset_path(&asset.path)?;
    }
    Ok(map(vec![
        ("schema", key(SCHEMA)),
        ("version", SteelDataValue::Integer(1)),
        (
            "engines",
            SteelDataValue::Vector(config.engines.iter().map(encode_engine).collect()),
        ),
        (
            "selected-engine-id",
            SteelDataValue::String(config.selected_engine_id.clone()),
        ),
        (
            "freecad-cmd",
            SteelDataValue::String(config.freecad_cmd.clone()),
        ),
        (
            "cad-text-font-path",
            SteelDataValue::String(config.cad_text_font_path.clone()),
        ),
        (
            "freecad-library-roots",
            strings(&config.freecad_library_roots),
        ),
        (
            "assets",
            SteelDataValue::Vector(config.assets.iter().map(encode_asset).collect()),
        ),
        (
            "microwave",
            config
                .microwave
                .as_ref()
                .map(encode_microwave)
                .unwrap_or(SteelDataValue::Nil),
        ),
        (
            "voice",
            map(vec![(
                "stt-language-code",
                SteelDataValue::String(config.voice.stt_language_code.clone()),
            )]),
        ),
        ("mcp", encode_mcp(&config.mcp)?),
        ("fem-compute", encode_fem_compute(&config.fem_compute)),
        (
            "has-seen-onboarding",
            SteelDataValue::Bool(config.has_seen_onboarding),
        ),
        (
            "connection-type",
            config
                .connection_type
                .as_deref()
                .map(connection)
                .transpose()?
                .unwrap_or(SteelDataValue::Nil),
        ),
        (
            "provider-models",
            map(vec![
                (
                    "codex",
                    SteelDataValue::String(config.provider_models.codex.clone()),
                ),
                (
                    "agy",
                    SteelDataValue::String(config.provider_models.agy.clone()),
                ),
            ]),
        ),
        (
            "jev-classifier",
            map(vec![
                (
                    "enabled",
                    SteelDataValue::Bool(config.jev_classifier.enabled),
                ),
                (
                    "api-key",
                    SteelDataValue::String(config.jev_classifier.api_key.clone()),
                ),
            ]),
        ),
        (
            "default-engine-kind",
            key(engine_kind(config.default_engine_kind)),
        ),
        (
            "default-source-language",
            key(source_language(config.default_source_language)),
        ),
        (
            "default-geometry-backend",
            key(geometry_backend(config.default_geometry_backend)),
        ),
        (
            "max-generation-attempts",
            SteelDataValue::Integer(i64::from(config.max_generation_attempts)),
        ),
        (
            "max-verify-attempts",
            SteelDataValue::Integer(i64::from(config.max_verify_attempts)),
        ),
        ("projects-root", opt_string(&config.projects_root)),
    ]))
}

fn strings(values: &[String]) -> SteelDataValue {
    SteelDataValue::Vector(values.iter().cloned().map(SteelDataValue::String).collect())
}
fn unique_ids<'a>(ids: impl Iterator<Item = &'a str>, field: &str) -> AppResult<()> {
    let mut seen = HashSet::new();
    for id in ids {
        if !seen.insert(id) {
            return Err(err(field, "duplicate id"));
        }
    }
    Ok(())
}
fn validate_asset_path(path: &str) -> AppResult<()> {
    if path.is_empty() || !Path::new(path).is_absolute() {
        Err(err("asset.path", "expected nonempty absolute path"))
    } else {
        Ok(())
    }
}
fn decode_engines(values: &[SteelDataValue]) -> AppResult<Vec<Engine>> {
    let values: Vec<Engine> = values.iter().map(decode_engine).collect::<AppResult<_>>()?;
    unique_ids(values.iter().map(|x| x.id.as_str()), "engines")?;
    Ok(values)
}
fn decode_assets(values: &[SteelDataValue]) -> AppResult<Vec<Asset>> {
    let values: Vec<Asset> = values.iter().map(decode_asset).collect::<AppResult<_>>()?;
    unique_ids(values.iter().map(|x| x.id.as_str()), "assets")?;
    Ok(values)
}
fn encode_engine(x: &Engine) -> SteelDataValue {
    let mut overrides: Vec<_> = x.vision_overrides.iter().collect();
    overrides.sort_by(|(a, _), (b, _)| a.as_bytes().cmp(b.as_bytes()));
    map(vec![
        ("id", SteelDataValue::String(x.id.clone())),
        ("name", SteelDataValue::String(x.name.clone())),
        ("provider", SteelDataValue::String(x.provider.clone())),
        ("api-key", SteelDataValue::String(x.api_key.clone())),
        ("model", SteelDataValue::String(x.model.clone())),
        ("light-model", SteelDataValue::String(x.light_model.clone())),
        ("base-url", SteelDataValue::String(x.base_url.clone())),
        ("enabled", SteelDataValue::Bool(x.enabled)),
        (
            "vision-overrides",
            SteelDataValue::Vector(
                overrides
                    .into_iter()
                    .map(|(id, cap)| {
                        map(vec![
                            ("model-id", SteelDataValue::String(id.clone())),
                            ("capability", key(vision(cap))),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}
fn encode_asset(x: &Asset) -> SteelDataValue {
    map(vec![
        ("id", SteelDataValue::String(x.id.clone())),
        ("name", SteelDataValue::String(x.name.clone())),
        ("path", SteelDataValue::String(x.path.clone())),
        ("format", SteelDataValue::String(x.format.clone())),
    ])
}
fn encode_microwave(x: &MicrowaveConfig) -> SteelDataValue {
    map(vec![
        ("hum-id", opt_string(&x.hum_id)),
        ("ding-id", opt_string(&x.ding_id)),
        ("muted", SteelDataValue::Bool(x.muted)),
    ])
}
fn encode_mcp(x: &McpConfig) -> AppResult<SteelDataValue> {
    let prompt_timeout_secs = i64::try_from(x.prompt_timeout_secs)
        .map_err(|_| err("prompt-timeout-secs", "integer outside accepted range"))?;
    Ok(map(vec![
        ("port", opt_int(x.port)),
        ("max-sessions", opt_int(x.max_sessions)),
        (
            "mode",
            key(match x.mode {
                McpMode::Passive => "passive",
                McpMode::Active => "active",
            }),
        ),
        ("primary-agent-id", opt_string(&x.primary_agent_id)),
        (
            "prompt-timeout-secs",
            SteelDataValue::Integer(prompt_timeout_secs),
        ),
        (
            "ecky-ast-authoring",
            SteelDataValue::Bool(x.ecky_ast_authoring),
        ),
        (
            "auto-agents",
            SteelDataValue::Vector(
                x.auto_agents
                    .iter()
                    .map(|a| {
                        map(vec![
                            ("id", SteelDataValue::String(a.id.clone())),
                            ("label", SteelDataValue::String(a.label.clone())),
                            ("cmd", SteelDataValue::String(a.cmd.clone())),
                            ("model", opt_string(&a.model)),
                            ("args", strings(&a.args)),
                            ("enabled", SteelDataValue::Bool(a.enabled)),
                        ])
                    })
                    .collect(),
            ),
        ),
    ]))
}

fn encode_fem_compute(x: &FemComputeConfig) -> SteelDataValue {
    map(vec![
        (
            "quality",
            key(match x.quality {
                FemComputeQuality::Draft => "draft",
                FemComputeQuality::Balanced => "balanced",
                FemComputeQuality::Fine => "fine",
            }),
        ),
        (
            "maximum-wall-time-minutes",
            SteelDataValue::Integer(i64::from(x.maximum_wall_time_minutes)),
        ),
        (
            "maximum-memory-mib",
            SteelDataValue::Integer(i64::from(x.maximum_memory_mib)),
        ),
        (
            "thread-count",
            SteelDataValue::Integer(i64::from(x.thread_count)),
        ),
    ])
}

fn decode_fem_compute(value: &SteelDataValue) -> AppResult<FemComputeConfig> {
    let fields = Fields::new(
        value,
        "fem-compute",
        &[
            "quality",
            "maximum-wall-time-minutes",
            "maximum-memory-mib",
            "thread-count",
        ],
    )?;
    let defaults = FemComputeConfig::default();
    let quality = match fields.optional("quality") {
        None => defaults.quality,
        Some(value) => match keyword(value, "fem-compute.quality")? {
            "draft" => FemComputeQuality::Draft,
            "balanced" => FemComputeQuality::Balanced,
            "fine" => FemComputeQuality::Fine,
            _ => return Err(err("fem-compute.quality", "unsupported keyword")),
        },
    };
    let maximum_wall_time_minutes = fields
        .optional("maximum-wall-time-minutes")
        .map(|value| unsigned(value, "fem-compute.maximum-wall-time-minutes", 24 * 60))
        .transpose()?
        .map(|value| value as u32)
        .unwrap_or(defaults.maximum_wall_time_minutes);
    let maximum_memory_mib = fields
        .optional("maximum-memory-mib")
        .map(|value| unsigned(value, "fem-compute.maximum-memory-mib", 1_048_576))
        .transpose()?
        .map(|value| value as u32)
        .unwrap_or(defaults.maximum_memory_mib);
    let thread_count = fields
        .optional("thread-count")
        .map(|value| unsigned(value, "fem-compute.thread-count", 256))
        .transpose()?
        .map(|value| value as u16)
        .unwrap_or(defaults.thread_count);
    if maximum_wall_time_minutes == 0 {
        return Err(err(
            "fem-compute.maximum-wall-time-minutes",
            "expected positive integer",
        ));
    }
    if maximum_memory_mib < 256 {
        return Err(err(
            "fem-compute.maximum-memory-mib",
            "expected at least 256 MiB",
        ));
    }
    Ok(FemComputeConfig {
        quality,
        maximum_wall_time_minutes,
        maximum_memory_mib,
        thread_count,
    })
}

pub fn decode_config(value: &SteelDataValue) -> AppResult<Config> {
    validate_steel_data(value).map_err(|_| err("root", "invalid Steel data value"))?;
    let root = Fields::new(
        value,
        "root",
        &[
            "schema",
            "version",
            "engines",
            "selected-engine-id",
            "freecad-cmd",
            "cad-text-font-path",
            "freecad-library-roots",
            "assets",
            "microwave",
            "voice",
            "mcp",
            "fem-compute",
            "has-seen-onboarding",
            "connection-type",
            "provider-models",
            "jev-classifier",
            "default-engine-kind",
            "default-source-language",
            "default-geometry-backend",
            "max-generation-attempts",
            "max-verify-attempts",
            "projects-root",
        ],
    )?;
    expect_keyword(root.required("schema")?, "schema", SCHEMA)?;
    expect_integer(root.required("version")?, "version", 1)?;
    let engines = decode_engines(vector(root.required("engines")?, "engines")?)?;
    let assets = root
        .optional("assets")
        .map(|v| decode_assets(vector(v, "assets")?))
        .transpose()?
        .unwrap_or_default();
    let config = Config {
        engines,
        selected_engine_id: string(root.required("selected-engine-id")?, "selected-engine-id")?,
        freecad_cmd: root
            .optional("freecad-cmd")
            .map(|v| string(v, "freecad-cmd"))
            .transpose()?
            .unwrap_or_default(),
        cad_text_font_path: root
            .optional("cad-text-font-path")
            .map(|v| string(v, "cad-text-font-path"))
            .transpose()?
            .unwrap_or_default(),
        freecad_library_roots: root
            .optional("freecad-library-roots")
            .map(|v| string_vector(v, "freecad-library-roots"))
            .transpose()?
            .unwrap_or_default(),
        assets,
        microwave: root
            .optional("microwave")
            .map(decode_optional_microwave)
            .transpose()?
            .flatten(),
        voice: root
            .optional("voice")
            .map(decode_voice)
            .transpose()?
            .unwrap_or_default(),
        mcp: root
            .optional("mcp")
            .map(decode_mcp)
            .transpose()?
            .unwrap_or_default(),
        fem_compute: root
            .optional("fem-compute")
            .map(decode_fem_compute)
            .transpose()?
            .unwrap_or_default(),
        has_seen_onboarding: root
            .optional("has-seen-onboarding")
            .map(|v| boolean(v, "has-seen-onboarding"))
            .transpose()?
            .unwrap_or(false),
        connection_type: root
            .optional("connection-type")
            .map(decode_connection)
            .transpose()?
            .flatten(),
        provider_models: root
            .optional("provider-models")
            .map(|value| -> AppResult<ProviderModels> {
                let fields = Fields::new(value, "provider-models", &["codex", "agy"])?;
                Ok(ProviderModels {
                    codex: fields
                        .optional("codex")
                        .map(|value| string(value, "provider-models.codex"))
                        .transpose()?
                        .unwrap_or_default(),
                    agy: fields
                        .optional("agy")
                        .map(|value| string(value, "provider-models.agy"))
                        .transpose()?
                        .unwrap_or_default(),
                })
            })
            .transpose()?
            .unwrap_or_default(),
        jev_classifier: root
            .optional("jev-classifier")
            .map(|value| -> AppResult<JevClassifierConfig> {
                let fields = Fields::new(value, "jev-classifier", &["enabled", "api-key"])?;
                Ok(JevClassifierConfig {
                    enabled: fields
                        .optional("enabled")
                        .map(|value| boolean(value, "jev-classifier.enabled"))
                        .transpose()?
                        .unwrap_or(false),
                    api_key: fields
                        .optional("api-key")
                        .map(|value| string(value, "jev-classifier.api-key"))
                        .transpose()?
                        .unwrap_or_default(),
                })
            })
            .transpose()?
            .unwrap_or_default(),
        default_engine_kind: root
            .optional("default-engine-kind")
            .map(decode_engine_kind)
            .transpose()?
            .unwrap_or(EngineKind::EckyIrV0),
        default_source_language: root
            .optional("default-source-language")
            .map(decode_source_language)
            .transpose()?
            .unwrap_or(SourceLanguage::EckyIrV0),
        default_geometry_backend: root
            .optional("default-geometry-backend")
            .map(decode_geometry_backend)
            .transpose()?
            .unwrap_or(GeometryBackend::EckyRust),
        max_generation_attempts: root
            .optional("max-generation-attempts")
            .map(|v| unsigned(v, "max-generation-attempts", u32::MAX as i64).map(|n| n as u32))
            .transpose()?
            .unwrap_or(3),
        max_verify_attempts: root
            .optional("max-verify-attempts")
            .map(|v| unsigned(v, "max-verify-attempts", u32::MAX as i64).map(|n| n as u32))
            .transpose()?
            .unwrap_or(2),
        projects_root: root
            .optional("projects-root")
            .map(|v| optional_string(v, "projects-root"))
            .transpose()?
            .flatten(),
    };
    for asset in &config.assets {
        validate_asset_path(&asset.path)?;
    }
    Ok(config)
}

fn decode_engine(v: &SteelDataValue) -> AppResult<Engine> {
    let f = Fields::new(
        v,
        "engine",
        &[
            "id",
            "name",
            "provider",
            "api-key",
            "model",
            "light-model",
            "base-url",
            "enabled",
            "vision-overrides",
        ],
    )?;
    let mut seen = HashSet::new();
    let mut overrides = HashMap::new();
    for item in f
        .optional("vision-overrides")
        .map(|x| vector(x, "vision-overrides"))
        .transpose()?
        .unwrap_or(&[])
    {
        let o = Fields::new(item, "vision-override", &["model-id", "capability"])?;
        let id = string(o.required("model-id")?, "model-id")?;
        if !seen.insert(id.clone()) {
            return Err(err("vision-overrides", "duplicate model-id"));
        }
        overrides.insert(id, decode_vision(o.required("capability")?)?);
    }
    Ok(Engine {
        id: string(f.required("id")?, "id")?,
        name: string(f.required("name")?, "name")?,
        provider: string(f.required("provider")?, "provider")?,
        api_key: string(f.required("api-key")?, "api-key")?,
        model: string(f.required("model")?, "model")?,
        light_model: f
            .optional("light-model")
            .map(|v| string(v, "light-model"))
            .transpose()?
            .unwrap_or_default(),
        base_url: string(f.required("base-url")?, "base-url")?,
        enabled: f
            .optional("enabled")
            .map(|v| boolean(v, "enabled"))
            .transpose()?
            .unwrap_or(true),
        vision_overrides: overrides,
    })
}
fn decode_asset(v: &SteelDataValue) -> AppResult<Asset> {
    let f = Fields::new(v, "asset", &["id", "name", "path", "format"])?;
    let path = string(f.required("path")?, "asset.path")?;
    validate_asset_path(&path)?;
    Ok(Asset {
        id: string(f.required("id")?, "asset.id")?,
        name: string(f.required("name")?, "asset.name")?,
        path,
        format: string(f.required("format")?, "asset.format")?,
    })
}
fn decode_optional_microwave(v: &SteelDataValue) -> AppResult<Option<MicrowaveConfig>> {
    if matches!(v, SteelDataValue::Nil) {
        return Ok(None);
    }
    let f = Fields::new(v, "microwave", &["hum-id", "ding-id", "muted"])?;
    Ok(Some(MicrowaveConfig {
        hum_id: f
            .optional("hum-id")
            .map(|v| optional_string(v, "hum-id"))
            .transpose()?
            .flatten(),
        ding_id: f
            .optional("ding-id")
            .map(|v| optional_string(v, "ding-id"))
            .transpose()?
            .flatten(),
        muted: f
            .optional("muted")
            .map(|v| boolean(v, "muted"))
            .transpose()?
            .unwrap_or(false),
    }))
}
fn decode_voice(v: &SteelDataValue) -> AppResult<VoiceConfig> {
    let f = Fields::new(v, "voice", &["stt-language-code"])?;
    Ok(VoiceConfig {
        stt_language_code: f
            .optional("stt-language-code")
            .map(|v| string(v, "stt-language-code"))
            .transpose()?
            .unwrap_or_else(|| "en-US".into()),
    })
}
fn decode_mcp(v: &SteelDataValue) -> AppResult<McpConfig> {
    let f = Fields::new(
        v,
        "mcp",
        &[
            "port",
            "max-sessions",
            "mode",
            "primary-agent-id",
            "prompt-timeout-secs",
            "ecky-ast-authoring",
            "auto-agents",
        ],
    )?;
    let agents: Vec<AutoAgent> = f
        .optional("auto-agents")
        .map(|v| vector(v, "auto-agents")?.iter().map(decode_agent).collect())
        .transpose()?
        .unwrap_or_default();
    unique_ids(agents.iter().map(|x| x.id.as_str()), "auto-agents")?;
    unique_ids(
        agents.iter().map(|x| x.label.as_str()),
        "auto-agents.labels",
    )?;
    let mode = f
        .optional("mode")
        .map(decode_mode)
        .transpose()?
        .unwrap_or(if agents.is_empty() {
            McpMode::Passive
        } else {
            McpMode::Active
        });
    Ok(McpConfig {
        port: f
            .optional("port")
            .map(|v| optional_unsigned(v, "port", u16::MAX as i64))
            .transpose()?
            .flatten()
            .map(|n| n as u16),
        max_sessions: f
            .optional("max-sessions")
            .map(|v| optional_unsigned(v, "max-sessions", u8::MAX as i64))
            .transpose()?
            .flatten()
            .map(|n| n as u8),
        mode,
        primary_agent_id: f
            .optional("primary-agent-id")
            .map(|v| optional_string(v, "primary-agent-id"))
            .transpose()?
            .flatten(),
        prompt_timeout_secs: f
            .optional("prompt-timeout-secs")
            .map(|v| unsigned(v, "prompt-timeout-secs", i64::MAX).map(|n| n as u64))
            .transpose()?
            .unwrap_or(1800),
        ecky_ast_authoring: f
            .optional("ecky-ast-authoring")
            .map(|v| boolean(v, "ecky-ast-authoring"))
            .transpose()?
            .unwrap_or(false),
        auto_agents: agents,
    })
}
fn decode_agent(v: &SteelDataValue) -> AppResult<AutoAgent> {
    let f = Fields::new(
        v,
        "auto-agent",
        &["id", "label", "cmd", "model", "args", "enabled"],
    )?;
    Ok(AutoAgent {
        id: string(f.required("id")?, "auto-agent.id")?,
        label: string(f.required("label")?, "auto-agent.label")?,
        cmd: string(f.required("cmd")?, "auto-agent.cmd")?,
        model: f
            .optional("model")
            .map(|v| optional_string(v, "auto-agent.model"))
            .transpose()?
            .flatten(),
        args: string_vector(f.required("args")?, "auto-agent.args")?,
        enabled: boolean(f.required("enabled")?, "auto-agent.enabled")?,
        start_on_demand: false,
    })
}

struct Fields<'a> {
    values: HashMap<&'a str, &'a SteelDataValue>,
    scope: &'static str,
}
impl<'a> Fields<'a> {
    fn new(v: &'a SteelDataValue, scope: &'static str, allowed: &[&str]) -> AppResult<Self> {
        let SteelDataValue::Map(entries) = v else {
            return Err(err(scope, "expected map"));
        };
        let mut values = HashMap::new();
        for (k, v) in entries {
            let Some(k) = k.strip_prefix(':') else {
                return Err(err(scope, "unknown key"));
            };
            if !allowed.contains(&k) {
                return Err(err(scope, "unknown key"));
            }
            if values.insert(k, v).is_some() {
                return Err(err(scope, "duplicate key"));
            }
        }
        Ok(Self { values, scope })
    }
    fn required(&self, k: &str) -> AppResult<&'a SteelDataValue> {
        self.values
            .get(k)
            .copied()
            .ok_or_else(|| err(self.scope, "missing required field"))
    }
    fn optional(&self, k: &str) -> Option<&'a SteelDataValue> {
        self.values.get(k).copied()
    }
}
fn string(v: &SteelDataValue, f: &str) -> AppResult<String> {
    if let SteelDataValue::String(x) = v {
        Ok(x.clone())
    } else {
        Err(err(f, "expected string"))
    }
}
fn optional_string(v: &SteelDataValue, f: &str) -> AppResult<Option<String>> {
    if matches!(v, SteelDataValue::Nil) {
        Ok(None)
    } else {
        string(v, f).map(Some)
    }
}
fn boolean(v: &SteelDataValue, f: &str) -> AppResult<bool> {
    if let SteelDataValue::Bool(x) = v {
        Ok(*x)
    } else {
        Err(err(f, "expected bool"))
    }
}
fn vector<'a>(v: &'a SteelDataValue, f: &str) -> AppResult<&'a [SteelDataValue]> {
    if let SteelDataValue::Vector(x) = v {
        Ok(x)
    } else {
        Err(err(f, "expected vector"))
    }
}
fn string_vector(v: &SteelDataValue, f: &str) -> AppResult<Vec<String>> {
    vector(v, f)?.iter().map(|x| string(x, f)).collect()
}
fn unsigned(v: &SteelDataValue, f: &str, max: i64) -> AppResult<i64> {
    match v {
        SteelDataValue::Integer(n) if *n >= 0 && *n <= max => Ok(*n),
        SteelDataValue::Integer(_) => Err(err(f, "integer outside accepted range")),
        _ => Err(err(f, "expected integer")),
    }
}
fn optional_unsigned(v: &SteelDataValue, f: &str, max: i64) -> AppResult<Option<i64>> {
    if matches!(v, SteelDataValue::Nil) {
        Ok(None)
    } else {
        unsigned(v, f, max).map(Some)
    }
}
fn expect_integer(v: &SteelDataValue, f: &str, want: i64) -> AppResult<()> {
    if matches!(v,SteelDataValue::Integer(n) if *n==want) {
        Ok(())
    } else {
        Err(err(f, "unsupported value"))
    }
}
fn expect_keyword(v: &SteelDataValue, f: &str, want: &str) -> AppResult<()> {
    if matches!(v,SteelDataValue::Keyword(n) if n.strip_prefix(':')==Some(want)) {
        Ok(())
    } else {
        Err(err(f, "unsupported value"))
    }
}
fn keyword<'a>(v: &'a SteelDataValue, f: &str) -> AppResult<&'a str> {
    if let SteelDataValue::Keyword(x) = v {
        x.strip_prefix(':')
            .ok_or_else(|| err(f, "expected keyword"))
    } else {
        Err(err(f, "expected keyword"))
    }
}
fn vision(v: &VisionCapability) -> &'static str {
    match v {
        VisionCapability::Auto => "auto",
        VisionCapability::Vision => "vision",
        VisionCapability::TextOnly => "text-only",
    }
}
fn decode_vision(v: &SteelDataValue) -> AppResult<VisionCapability> {
    match keyword(v, "capability")? {
        "auto" => Ok(VisionCapability::Auto),
        "vision" => Ok(VisionCapability::Vision),
        "text-only" => Ok(VisionCapability::TextOnly),
        _ => Err(err("capability", "unsupported keyword")),
    }
}
fn connection(s: &str) -> AppResult<SteelDataValue> {
    match s {
        "api_key" => Ok(key("api-key")),
        "mcp" => Ok(key("mcp")),
        _ => {
            let provider = s
                .strip_prefix("provider:")
                .filter(|provider| {
                    !provider.is_empty()
                        && provider
                            .chars()
                            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
                })
                .ok_or_else(|| err("connection-type", "unsupported value"))?;
            Ok(key(&format!("provider/{provider}")))
        }
    }
}
fn decode_connection(v: &SteelDataValue) -> AppResult<Option<String>> {
    if matches!(v, SteelDataValue::Nil) {
        return Ok(None);
    }
    let value = keyword(v, "connection-type")?;
    match value {
        "api-key" => Ok(Some("api_key".into())),
        "mcp" => Ok(Some("mcp".into())),
        _ => value
            .strip_prefix("provider/")
            .filter(|provider| {
                !provider.is_empty()
                    && provider
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
            })
            .map(|provider| Some(format!("provider:{provider}")))
            .ok_or_else(|| err("connection-type", "unsupported keyword")),
    }
}
fn engine_kind(v: EngineKind) -> &'static str {
    match v {
        EngineKind::Freecad => "freecad",
        EngineKind::EckyIrV0 => "ecky",
        EngineKind::Build123d => "build123d",
    }
}
fn source_language(v: SourceLanguage) -> &'static str {
    match v {
        SourceLanguage::LegacyPython => "legacy-python",
        SourceLanguage::EckyIrV0 => "ecky",
        SourceLanguage::Build123d => "build123d",
    }
}
fn geometry_backend(v: GeometryBackend) -> &'static str {
    match v {
        GeometryBackend::Freecad => "freecad",
        GeometryBackend::Build123d => "build123d",
        GeometryBackend::EckyRust => "mesh",
    }
}
fn decode_engine_kind(v: &SteelDataValue) -> AppResult<EngineKind> {
    match keyword(v, "default-engine-kind")? {
        "freecad" => Ok(EngineKind::Freecad),
        "ecky" => Ok(EngineKind::EckyIrV0),
        "build123d" => Ok(EngineKind::Build123d),
        _ => Err(err("default-engine-kind", "unsupported keyword")),
    }
}
fn decode_source_language(v: &SteelDataValue) -> AppResult<SourceLanguage> {
    match keyword(v, "default-source-language")? {
        "legacy-python" => Ok(SourceLanguage::LegacyPython),
        "ecky" => Ok(SourceLanguage::EckyIrV0),
        "build123d" => Ok(SourceLanguage::Build123d),
        _ => Err(err("default-source-language", "unsupported keyword")),
    }
}
fn decode_geometry_backend(v: &SteelDataValue) -> AppResult<GeometryBackend> {
    match keyword(v, "default-geometry-backend")? {
        "freecad" => Ok(GeometryBackend::Freecad),
        "build123d" => Ok(GeometryBackend::Build123d),
        "mesh" => Ok(GeometryBackend::EckyRust),
        _ => Err(err("default-geometry-backend", "unsupported keyword")),
    }
}
fn decode_mode(v: &SteelDataValue) -> AppResult<McpMode> {
    match keyword(v, "mode")? {
        "passive" => Ok(McpMode::Passive),
        "active" => Ok(McpMode::Active),
        _ => Err(err("mode", "unsupported keyword")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn minimum() -> Config {
        Config {
            engines: vec![],
            selected_engine_id: "selected".into(),
            freecad_cmd: String::new(),
            cad_text_font_path: String::new(),
            freecad_library_roots: vec![],
            assets: vec![],
            microwave: None,
            voice: VoiceConfig::default(),
            mcp: McpConfig::default(),
            fem_compute: FemComputeConfig::default(),
            has_seen_onboarding: false,
            connection_type: None,
            provider_models: ProviderModels::default(),
            jev_classifier: Default::default(),
            default_engine_kind: EngineKind::EckyIrV0,
            default_source_language: SourceLanguage::EckyIrV0,
            default_geometry_backend: GeometryBackend::Freecad,
            max_generation_attempts: 3,
            max_verify_attempts: 2,
            projects_root: None,
        }
    }
    #[test]
    fn bdd_canonical_round_trip_and_redacts_secrets() {
        let mut config = minimum();
        config.has_seen_onboarding = true;
        config.provider_models = ProviderModels {
            codex: "gpt-5.6".into(),
            agy: "claude-sonnet-4-6".into(),
        };
        let edn = encode_config(&config).unwrap();
        let rendered = crate::steel_data::write_steel_data(&edn).unwrap();
        assert!(rendered.contains(":provider-models"));
        assert!(rendered.contains("gpt-5.6"));
        assert_eq!(decode_config(&edn).unwrap(), config);
    }
    #[test]
    fn bdd_fem_compute_policy_round_trips_without_internal_solve_budget() {
        let mut config = minimum();
        config.fem_compute = FemComputeConfig {
            quality: FemComputeQuality::Fine,
            maximum_wall_time_minutes: 45,
            maximum_memory_mib: 12_288,
            thread_count: 8,
        };
        let encoded = encode_config(&config).unwrap();
        let rendered = crate::steel_data::write_steel_data(&encoded).unwrap();

        assert!(rendered.contains(":fem-compute"));
        assert!(!rendered.contains("solve-count"));
        assert_eq!(decode_config(&encoded).unwrap(), config);
    }
    #[test]
    fn provider_adapter_round_trips_as_namespaced_connection_keyword() {
        let mut config = minimum();
        config.connection_type = Some("provider:codex".into());
        let edn = encode_config(&config).unwrap();
        assert_eq!(decode_config(&edn).unwrap(), config);
    }
    #[test]
    fn bdd_rejects_unknown_alias_and_unsigned_overflow() {
        let base = encode_config(&minimum()).unwrap();
        for (key, value) in [
            (":unknown", SteelDataValue::Nil),
            (":selectedEngineId", SteelDataValue::String("bad".into())),
            (":max-generation-attempts", SteelDataValue::Integer(-1)),
            (
                ":max-generation-attempts",
                SteelDataValue::Integer(i64::from(u32::MAX) + 1),
            ),
        ] {
            let mut edn = base.clone();
            let SteelDataValue::Map(root) = &mut edn else {
                unreachable!()
            };
            if key == ":max-generation-attempts" {
                root.retain(|(k, _)| k != key)
            }
            root.push((key.into(), value));
            assert!(decode_config(&edn).is_err());
        }
    }
    #[test]
    fn bdd_absent_nested_defaults_and_nil_options() {
        let mut edn = encode_config(&minimum()).unwrap();
        let SteelDataValue::Map(root) = &mut edn else {
            unreachable!()
        };
        root.retain(|(k, _)| {
            !matches!(
                k.as_str(),
                ":freecad-cmd" | ":microwave" | ":voice" | ":mcp" | ":max-verify-attempts"
            )
        });
        let decoded = decode_config(&edn).unwrap();
        assert_eq!(decoded.freecad_cmd, "");
        assert!(decoded.microwave.is_none());
        assert_eq!(decoded.voice, VoiceConfig::default());
        assert_eq!(decoded.max_verify_attempts, 2);
        let encoded = encode_config(&decoded).unwrap();
        assert!(
            matches!(encoded,SteelDataValue::Map(ref m) if m.iter().any(|(k,v)|k==":microwave"&&matches!(v,SteelDataValue::Nil)))
        );
    }
    #[test]
    fn bdd_rejects_deprecated_ids_paths_and_oversized_manual_values() {
        let mut config = minimum();
        config.engines = vec![
            Engine {
                id: "same".into(),
                name: "a".into(),
                provider: "p".into(),
                api_key: "secret".into(),
                model: "m".into(),
                light_model: "".into(),
                base_url: "".into(),
                enabled: true,
                vision_overrides: HashMap::new()
            };
            2
        ];
        assert!(encode_config(&config).is_err());
        config.engines.clear();
        config.assets.push(Asset {
            id: "asset".into(),
            name: "n".into(),
            path: "relative-secret-path".into(),
            format: "stl".into(),
        });
        let e = encode_config(&config).unwrap_err().to_string();
        assert!(!e.contains("relative-secret-path"));
        config.assets.clear();
        config.mcp.auto_agents.push(AutoAgent {
            id: "agent".into(),
            label: "same label".into(),
            cmd: "secret-cmd".into(),
            model: None,
            args: vec!["secret-arg".into()],
            enabled: true,
            start_on_demand: true,
        });
        assert!(encode_config(&config).is_err());
        assert!(decode_config(&SteelDataValue::Vector(vec![SteelDataValue::Nil; 10_001])).is_err());
    }
    #[test]
    fn bdd_legacy_normalization_is_static_and_enables_canonical_encode() {
        let mut config = minimum();
        config.mcp.auto_agents.push(AutoAgent {
            id: "id".into(),
            label: "label".into(),
            cmd: "secret-cmd".into(),
            model: None,
            args: vec!["secret-arg".into()],
            enabled: true,
            start_on_demand: true,
        });
        let warnings = normalize_legacy_config_for_edn(&mut config);
        assert_eq!(
            warnings,
            vec![ConfigNormalizationWarning {
                code: CONFIG_DEPRECATED_START_ON_DEMAND_DROPPED,
                field: "mcp.autoAgents[].startOnDemand"
            }]
        );
        assert!(!format!("{warnings:?}").contains("secret"));
        assert!(
            !decode_config(&encode_config(&config).unwrap())
                .unwrap()
                .mcp
                .auto_agents[0]
                .start_on_demand
        );
    }
    #[test]
    fn bdd_target_width_boundaries_and_prompt_encode_overflow() {
        let mut config = minimum();
        config.mcp.port = Some(u16::MAX);
        config.mcp.max_sessions = Some(u8::MAX);
        config.mcp.prompt_timeout_secs = i64::MAX as u64;
        config.max_generation_attempts = u32::MAX;
        assert_eq!(
            decode_config(&encode_config(&config).unwrap()).unwrap(),
            config
        );
        config.mcp.prompt_timeout_secs = (i64::MAX as u64) + 1;
        assert!(encode_config(&config).is_err());
    }
    #[test]
    fn bdd_direct_value_validator_enforces_representative_limits() {
        use crate::steel_data::{validate_steel_data, MAX_STRING_BYTES};
        assert!(
            validate_steel_data(&SteelDataValue::String("x".repeat(MAX_STRING_BYTES + 1))).is_err()
        );
        assert!(validate_steel_data(&SteelDataValue::Map(vec![
            (":a".into(), SteelDataValue::Nil),
            (":a".into(), SteelDataValue::Nil)
        ]))
        .is_err());
        let mut deep = SteelDataValue::Nil;
        for _ in 0..66 {
            deep = SteelDataValue::Vector(vec![deep]);
        }
        assert!(validate_steel_data(&deep).is_err());
        assert!(
            validate_steel_data(&SteelDataValue::Vector(vec![SteelDataValue::Nil; 100_001]))
                .is_err()
        );
    }
}
