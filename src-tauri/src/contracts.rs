use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::{BTreeMap, HashMap, HashSet};

pub type AppResult<T> = Result<T, AppError>;
pub type DesignParams = BTreeMap<String, ParamValue>;
pub const GENIE_TRAITS_VERSION: u8 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AppErrorCode {
    Validation,
    NotFound,
    Provider,
    Persistence,
    Render,
    Parse,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: AppErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

impl AppError {
    pub fn new(code: AppErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(
        code: AppErrorCode,
        message: impl Into<String>,
        details: impl Into<String>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            details: Some(details.into()),
        }
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Validation, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::NotFound, message)
    }

    pub fn provider(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Provider, message)
    }

    pub fn persistence(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Persistence, message)
    }

    pub fn render(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Render, message)
    }

    pub fn parse(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Parse, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Internal, message)
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.details.as_deref() {
            Some(details) if !details.trim().is_empty() => {
                write!(f, "{}: {}", self.message, details)
            }
            _ => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for AppError {}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Engine {
    pub id: String,
    pub name: String,
    pub provider: String,
    #[serde(alias = "api_key")]
    pub api_key: String,
    pub model: String,
    #[serde(default, alias = "light_model")]
    pub light_model: String,
    #[serde(alias = "base_url")]
    pub base_url: String,
    #[serde(alias = "system_prompt")]
    pub system_prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub id: String,
    pub name: String,
    pub path: String,
    pub format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MicrowaveConfig {
    #[serde(default, alias = "hum_id")]
    pub hum_id: Option<String>,
    #[serde(default, alias = "ding_id")]
    pub ding_id: Option<String>,
    #[serde(default)]
    pub muted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub engines: Vec<Engine>,
    #[serde(alias = "selected_engine_id")]
    pub selected_engine_id: String,
    #[serde(default, alias = "freecad_cmd")]
    pub freecad_cmd: String,
    #[serde(default)]
    pub assets: Vec<Asset>,
    #[serde(default)]
    pub microwave: Option<MicrowaveConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum EyeStyle {
    Dot,
    Bar,
    Slant,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GenieTraits {
    #[serde(default = "default_genie_traits_version")]
    pub version: u8,
    pub seed: u32,
    pub color_hue: f64,
    pub vertex_count: u8,
    pub radius_base: f64,
    pub stretch_y: f64,
    pub asymmetry: f64,
    pub chord_skip: u8,
    pub jitter_scale: f64,
    pub pulse_scale: f64,
    pub hover_scale: f64,
    pub warp_scale: f64,
    pub glow_hue_shift: f64,
    pub eye_style: EyeStyle,
    pub eye_spacing: f64,
    pub eye_size: f64,
    pub mouth_curve: f64,
    pub thinking_bias: f64,
    pub repair_bias: f64,
    pub render_bias: f64,
    pub expressiveness: f64,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct LegacyGenieTraitsV1 {
    #[serde(default)]
    seed: Option<u32>,
    #[serde(default)]
    color_hue: Option<f64>,
    #[serde(default)]
    vertex_count: Option<u8>,
    #[serde(default)]
    jitter_scale: Option<f64>,
    #[serde(default)]
    pulse_scale: Option<f64>,
}

#[derive(Debug, Clone)]
struct GeneRng {
    state: u64,
}

impl GeneRng {
    fn new(seed: u64) -> Self {
        Self {
            state: seed ^ 0x9E37_79B9_7F4A_7C15,
        }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn unit(&mut self) -> f64 {
        ((self.next_u64() >> 11) as f64) / ((1u64 << 53) as f64)
    }

    fn range_f64(&mut self, min: f64, max: f64) -> f64 {
        min + (max - min) * self.unit()
    }

    fn range_u8(&mut self, min: u8, max_inclusive: u8) -> u8 {
        min + (self.next_u64() % (u64::from(max_inclusive - min) + 1)) as u8
    }

    fn eye_style(&mut self) -> EyeStyle {
        match self.range_u8(0, 2) {
            0 => EyeStyle::Dot,
            1 => EyeStyle::Bar,
            _ => EyeStyle::Slant,
        }
    }
}

fn default_genie_traits_version() -> u8 {
    GENIE_TRAITS_VERSION
}

fn clamp_f64(value: f64, min: f64, max: f64) -> f64 {
    value.clamp(min, max)
}

fn normalize_hue(value: f64) -> f64 {
    value.rem_euclid(360.0)
}

pub fn derive_thread_seed(thread_id: &str) -> u32 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in thread_id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    let seed = ((hash >> 32) ^ hash) as u32;
    if seed == 0 {
        1
    } else {
        seed
    }
}

impl GenieTraits {
    pub fn from_seed(seed: u32) -> Self {
        let mut rng = GeneRng::new(u64::from(seed));
        Self {
            version: GENIE_TRAITS_VERSION,
            seed,
            color_hue: rng.range_f64(0.0, 360.0),
            vertex_count: rng.range_u8(10, 24),
            radius_base: rng.range_f64(25.0, 34.0),
            stretch_y: rng.range_f64(0.90, 1.06),
            asymmetry: rng.range_f64(0.88, 1.14),
            chord_skip: rng.range_u8(2, 6),
            jitter_scale: rng.range_f64(0.70, 1.45),
            pulse_scale: rng.range_f64(0.70, 1.35),
            hover_scale: rng.range_f64(0.80, 1.60),
            warp_scale: rng.range_f64(0.35, 1.25),
            glow_hue_shift: rng.range_f64(-32.0, 32.0),
            eye_style: rng.eye_style(),
            eye_spacing: rng.range_f64(15.0, 22.5),
            eye_size: rng.range_f64(2.00, 3.60),
            mouth_curve: rng.range_f64(0.60, 2.60),
            thinking_bias: rng.range_f64(0.20, 1.00),
            repair_bias: rng.range_f64(0.20, 1.00),
            render_bias: rng.range_f64(0.20, 1.00),
            expressiveness: rng.range_f64(0.35, 1.00),
        }
    }

    pub fn normalized(mut self) -> Self {
        if self.version == 0 {
            self.version = GENIE_TRAITS_VERSION;
        }
        if self.seed == 0 {
            self.seed = 1;
        }
        self.color_hue = normalize_hue(self.color_hue);
        self.vertex_count = self.vertex_count.clamp(10, 24);
        self.radius_base = clamp_f64(self.radius_base, 25.0, 34.0);
        self.stretch_y = clamp_f64(self.stretch_y, 0.90, 1.06);
        self.asymmetry = clamp_f64(self.asymmetry, 0.88, 1.14);
        self.chord_skip = self.chord_skip.clamp(2, 6);
        self.jitter_scale = clamp_f64(self.jitter_scale, 0.70, 1.45);
        self.pulse_scale = clamp_f64(self.pulse_scale, 0.70, 1.35);
        self.hover_scale = clamp_f64(self.hover_scale, 0.80, 1.60);
        self.warp_scale = clamp_f64(self.warp_scale, 0.35, 1.25);
        self.glow_hue_shift = clamp_f64(self.glow_hue_shift, -32.0, 32.0);
        self.eye_spacing = clamp_f64(self.eye_spacing, 15.0, 22.5);
        self.eye_size = clamp_f64(self.eye_size, 2.00, 3.60);
        self.mouth_curve = clamp_f64(self.mouth_curve, 0.60, 2.60);
        self.thinking_bias = clamp_f64(self.thinking_bias, 0.20, 1.00);
        self.repair_bias = clamp_f64(self.repair_bias, 0.20, 1.00);
        self.render_bias = clamp_f64(self.render_bias, 0.20, 1.00);
        self.expressiveness = clamp_f64(self.expressiveness, 0.35, 1.00);
        self
    }

    fn from_legacy(legacy: LegacyGenieTraitsV1, thread_id: Option<&str>) -> Self {
        let seed = legacy
            .seed
            .filter(|value| *value != 0)
            .unwrap_or_else(|| derive_thread_seed(thread_id.unwrap_or("legacy-thread")));
        let mut traits = Self::from_seed(seed);
        if let Some(color_hue) = legacy.color_hue {
            traits.color_hue = normalize_hue(color_hue);
        }
        if let Some(vertex_count) = legacy.vertex_count {
            traits.vertex_count = vertex_count.clamp(10, 24);
        }
        if let Some(jitter_scale) = legacy.jitter_scale {
            traits.jitter_scale = clamp_f64(jitter_scale, 0.70, 1.45);
        }
        if let Some(pulse_scale) = legacy.pulse_scale {
            traits.pulse_scale = clamp_f64(pulse_scale, 0.70, 1.35);
        }
        traits.normalized()
    }
}

pub fn decode_genie_traits_json(raw: &str, thread_id: Option<&str>) -> Option<GenieTraits> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    if value.get("version").is_some() {
        serde_json::from_value::<GenieTraits>(value)
            .ok()
            .map(GenieTraits::normalized)
    } else {
        serde_json::from_value::<LegacyGenieTraitsV1>(value)
            .ok()
            .map(|legacy| GenieTraits::from_legacy(legacy, thread_id))
    }
}

pub fn upgraded_or_default_genie_traits(thread_id: &str, raw: Option<&str>) -> GenieTraits {
    raw.and_then(|json| decode_genie_traits_json(json, Some(thread_id)))
        .unwrap_or_else(|| GenieTraits::from_seed(derive_thread_seed(thread_id)))
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiSpec {
    #[serde(default)]
    pub fields: Vec<UiField>,
}

impl Default for UiSpec {
    fn default() -> Self {
        Self { fields: Vec::new() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase", untagged)]
pub enum ParamValue {
    String(String),
    Number(f64),
    Boolean(bool),
    Null,
}

impl ParamValue {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::String(_) => "string",
            Self::Number(_) => "number",
            Self::Boolean(_) => "boolean",
            Self::Null => "null",
        }
    }

    pub fn matches_select_value(&self, value: &SelectValue) -> bool {
        match (self, value) {
            (Self::String(left), SelectValue::String(right)) => left == right,
            (Self::Number(left), SelectValue::Number(right)) => left == right,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase", untagged)]
pub enum SelectValue {
    String(String),
    Number(f64),
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SelectOption {
    pub label: String,
    pub value: SelectValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum UiField {
    Range {
        key: String,
        #[serde(default)]
        label: String,
        #[serde(default)]
        min: Option<f64>,
        #[serde(default)]
        max: Option<f64>,
        #[serde(default)]
        step: Option<f64>,
        #[serde(default, rename = "minFrom", alias = "min_from")]
        min_from: Option<String>,
        #[serde(default, rename = "maxFrom", alias = "max_from")]
        max_from: Option<String>,
        #[serde(default, alias = "freezed")]
        frozen: bool,
    },
    Number {
        key: String,
        #[serde(default)]
        label: String,
        #[serde(default)]
        min: Option<f64>,
        #[serde(default)]
        max: Option<f64>,
        #[serde(default)]
        step: Option<f64>,
        #[serde(default, rename = "minFrom", alias = "min_from")]
        min_from: Option<String>,
        #[serde(default, rename = "maxFrom", alias = "max_from")]
        max_from: Option<String>,
        #[serde(default, alias = "freezed")]
        frozen: bool,
    },
    Select {
        key: String,
        #[serde(default)]
        label: String,
        #[serde(default)]
        options: Vec<SelectOption>,
        #[serde(default, alias = "freezed")]
        frozen: bool,
    },
    Checkbox {
        key: String,
        #[serde(default)]
        label: String,
        #[serde(default, alias = "freezed")]
        frozen: bool,
    },
}

impl UiField {
    pub fn key(&self) -> &str {
        match self {
            Self::Range { key, .. }
            | Self::Number { key, .. }
            | Self::Select { key, .. }
            | Self::Checkbox { key, .. } => key,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Range { label, .. }
            | Self::Number { label, .. }
            | Self::Select { label, .. }
            | Self::Checkbox { label, .. } => label,
        }
    }

    pub fn frozen(&self) -> bool {
        match self {
            Self::Range { frozen, .. }
            | Self::Number { frozen, .. }
            | Self::Select { frozen, .. }
            | Self::Checkbox { frozen, .. } => *frozen,
        }
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, Self::Range { .. } | Self::Number { .. })
    }

    pub fn validate_value(&self, value: &ParamValue) -> AppResult<()> {
        match self {
            Self::Range { key, .. } | Self::Number { key, .. } => match value {
                ParamValue::Number(_) => Ok(()),
                other => Err(AppError::validation(format!(
                    "Parameter '{}' must be a number, received {}.",
                    key,
                    other.kind()
                ))),
            },
            Self::Checkbox { key, .. } => match value {
                ParamValue::Boolean(_) => Ok(()),
                other => Err(AppError::validation(format!(
                    "Parameter '{}' must be a boolean, received {}.",
                    key,
                    other.kind()
                ))),
            },
            Self::Select { key, options, .. } => match value {
                ParamValue::String(_) | ParamValue::Number(_) => {
                    if options.is_empty()
                        || options
                            .iter()
                            .any(|option| value.matches_select_value(&option.value))
                    {
                        Ok(())
                    } else {
                        Err(AppError::validation(format!(
                            "Parameter '{}' must match one of the declared select options.",
                            key
                        )))
                    }
                }
                other => Err(AppError::validation(format!(
                    "Parameter '{}' must be a string or number for a select field, received {}.",
                    key,
                    other.kind()
                ))),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InteractionMode {
    Design,
    Question,
}

impl InteractionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Design => "design",
            Self::Question => "question",
        }
    }
}

impl std::str::FromStr for InteractionMode {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "design" => Ok(Self::Design),
            "question" => Ok(Self::Question),
            other => Err(AppError::validation(format!(
                "Unknown interaction mode '{}'.",
                other
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DesignOutput {
    #[serde(default = "default_title")]
    pub title: String,
    #[serde(default = "default_version_name", alias = "version_name")]
    pub version_name: String,
    #[serde(default)]
    pub response: String,
    #[serde(default = "default_interaction_mode", alias = "interaction_mode")]
    pub interaction_mode: InteractionMode,
    #[serde(alias = "macro_code")]
    pub macro_code: String,
    #[serde(default, alias = "ui_spec")]
    pub ui_spec: UiSpec,
    #[serde(default, alias = "initial_params")]
    pub initial_params: DesignParams,
}

fn default_title() -> String {
    "Untitled Design".to_string()
}

fn default_version_name() -> String {
    "V1".to_string()
}

fn default_interaction_mode() -> InteractionMode {
    InteractionMode::Design
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
}

impl MessageRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }
}

impl std::str::FromStr for MessageRole {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "user" => Ok(Self::User),
            "assistant" => Ok(Self::Assistant),
            other => Err(AppError::validation(format!(
                "Unknown message role '{}'.",
                other
            ))),
        }
    }
}

impl ToSql for MessageRole {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(self.as_str().into())
    }
}

impl FromSql for MessageRole {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let raw = value.as_str()?;
        raw.parse()
            .map_err(|err: AppError| FromSqlError::Other(Box::new(err)))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MessageStatus {
    Pending,
    Success,
    Error,
    Discarded,
}

impl MessageStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Success => "success",
            Self::Error => "error",
            Self::Discarded => "discarded",
        }
    }
}

impl std::str::FromStr for MessageStatus {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "pending" => Ok(Self::Pending),
            "success" => Ok(Self::Success),
            "error" => Ok(Self::Error),
            "discarded" => Ok(Self::Discarded),
            other => Err(AppError::validation(format!(
                "Unknown message status '{}'.",
                other
            ))),
        }
    }
}

impl ToSql for MessageStatus {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(self.as_str().into())
    }
}

impl FromSql for MessageStatus {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let raw = value.as_str()?;
        raw.parse()
            .map_err(|err: AppError| FromSqlError::Other(Box::new(err)))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub role: MessageRole,
    pub content: String,
    pub status: MessageStatus,
    #[serde(default)]
    pub output: Option<DesignOutput>,
    #[serde(default)]
    pub usage: Option<UsageSummary>,
    #[serde(default)]
    pub artifact_bundle: Option<ArtifactBundle>,
    #[serde(default)]
    pub model_manifest: Option<ModelManifest>,
    #[serde(default, alias = "image_data")]
    pub image_data: Option<String>,
    #[serde(default, alias = "attachment_images")]
    pub attachment_images: Vec<String>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    pub messages: Vec<Message>,
    #[serde(alias = "updated_at")]
    pub updated_at: u64,
    #[serde(default, alias = "genie_traits")]
    pub genie_traits: Option<GenieTraits>,
    #[serde(default, alias = "version_count")]
    pub version_count: usize,
    #[serde(default, alias = "pending_count")]
    pub pending_count: usize,
    #[serde(default, alias = "error_count")]
    pub error_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadReference {
    pub id: String,
    #[serde(alias = "thread_id")]
    pub thread_id: String,
    #[serde(default, alias = "source_message_id")]
    pub source_message_id: Option<String>,
    pub ordinal: i64,
    pub kind: String,
    pub name: String,
    pub content: String,
    pub summary: String,
    pub pinned: bool,
    #[serde(alias = "created_at")]
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AttachmentKind {
    Image,
    Cad,
}

impl AttachmentKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Cad => "cad",
        }
    }
}

impl std::str::FromStr for AttachmentKind {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "image" => Ok(Self::Image),
            "cad" => Ok(Self::Cad),
            other => Err(AppError::validation(format!(
                "Unknown attachment kind '{}'.",
                other
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub path: String,
    pub name: String,
    pub explanation: String,
    #[serde(alias = "type")]
    pub kind: AttachmentKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GenerateOutput {
    pub design: DesignOutput,
    pub thread_id: String,
    pub message_id: String,
    #[serde(default)]
    pub usage: Option<UsageSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommitOutput {
    pub thread_id: String,
    pub message_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IntentDecision {
    #[serde(alias = "intent_mode")]
    pub intent_mode: String,
    pub confidence: f32,
    pub response: String,
    #[serde(default)]
    pub usage: Option<UsageSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageSegment {
    pub stage: String,
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub cached_input_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
    #[serde(default)]
    pub estimated_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub cached_input_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
    #[serde(default)]
    pub estimated_cost_usd: Option<f64>,
    #[serde(default)]
    pub segments: Vec<UsageSegment>,
}

impl UsageSummary {
    pub fn empty() -> Self {
        Self {
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            cached_input_tokens: 0,
            reasoning_tokens: 0,
            estimated_cost_usd: None,
            segments: Vec::new(),
        }
    }

    pub fn from_segment(segment: UsageSegment) -> Self {
        Self {
            input_tokens: segment.input_tokens,
            output_tokens: segment.output_tokens,
            total_tokens: segment.total_tokens,
            cached_input_tokens: segment.cached_input_tokens,
            reasoning_tokens: segment.reasoning_tokens,
            estimated_cost_usd: segment.estimated_cost_usd,
            segments: vec![segment],
        }
    }

    pub fn merge(&self, other: &Self) -> Self {
        let estimated_cost_usd = match (self.estimated_cost_usd, other.estimated_cost_usd) {
            (Some(left), Some(right)) => Some(left + right),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        };

        let mut segments = self.segments.clone();
        segments.extend(other.segments.clone());

        Self {
            input_tokens: self.input_tokens + other.input_tokens,
            output_tokens: self.output_tokens + other.output_tokens,
            total_tokens: self.total_tokens + other.total_tokens,
            cached_input_tokens: self.cached_input_tokens + other.cached_input_tokens,
            reasoning_tokens: self.reasoning_tokens + other.reasoning_tokens,
            estimated_cost_usd,
            segments,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QuestionReply {
    pub thread_id: String,
    pub response: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LastDesignSnapshot {
    #[serde(default)]
    pub design: Option<DesignOutput>,
    #[serde(default, alias = "thread_id")]
    pub thread_id: Option<String>,
    #[serde(default, alias = "message_id")]
    pub message_id: Option<String>,
    #[serde(default, alias = "artifact_bundle")]
    pub artifact_bundle: Option<ArtifactBundle>,
    #[serde(default, alias = "model_manifest")]
    pub model_manifest: Option<ModelManifest>,
    #[serde(default, alias = "selected_part_id")]
    pub selected_part_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeletedMessage {
    pub id: String,
    #[serde(alias = "thread_id")]
    pub thread_id: String,
    pub thread_title: String,
    pub role: MessageRole,
    pub content: String,
    #[serde(default)]
    pub output: Option<DesignOutput>,
    #[serde(default)]
    pub usage: Option<UsageSummary>,
    #[serde(default)]
    pub artifact_bundle: Option<ArtifactBundle>,
    #[serde(default)]
    pub model_manifest: Option<ModelManifest>,
    pub timestamp: u64,
    #[serde(default, alias = "image_data")]
    pub image_data: Option<String>,
    #[serde(default, alias = "attachment_images")]
    pub attachment_images: Vec<String>,
    pub deleted_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ParsedParamsResult {
    pub fields: Vec<UiField>,
    pub params: DesignParams,
}

pub const MODEL_RUNTIME_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ModelSourceKind {
    Generated,
    ImportedFcstd,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ViewerAssetFormat {
    Stl,
    Gltf,
    Glb,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SelectionTargetKind {
    Part,
    Object,
    Group,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EnrichmentStatus {
    None,
    Pending,
    Accepted,
    Rejected,
}

impl Default for EnrichmentStatus {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ControlPrimitiveKind {
    Number,
    Toggle,
    Choice,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ControlRelationMode {
    Mirror,
    Scale,
    Offset,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ControlViewScope {
    Global,
    Part,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ControlViewSource {
    Generated,
    Inherited,
    Llm,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AdvisorySeverity {
    Info,
    Warning,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AdvisoryCondition {
    Always,
    Below,
    Above,
}

impl Default for AdvisoryCondition {
    fn default() -> Self {
        Self::Always
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ViewerAsset {
    pub part_id: String,
    pub node_id: String,
    pub object_name: String,
    pub label: String,
    pub path: String,
    pub format: ViewerAssetFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactBundle {
    #[serde(default = "default_model_runtime_schema_version")]
    pub schema_version: u32,
    pub model_id: String,
    pub source_kind: ModelSourceKind,
    pub content_hash: String,
    #[serde(default = "default_artifact_version")]
    pub artifact_version: u32,
    pub fcstd_path: String,
    pub manifest_path: String,
    #[serde(default)]
    pub macro_path: Option<String>,
    pub preview_stl_path: String,
    #[serde(default)]
    pub viewer_assets: Vec<ViewerAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManifestBounds {
    pub x_min: f64,
    pub y_min: f64,
    pub z_min: f64,
    pub x_max: f64,
    pub y_max: f64,
    pub z_max: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DocumentMetadata {
    pub document_name: String,
    pub document_label: String,
    #[serde(default)]
    pub source_path: Option<String>,
    #[serde(default)]
    pub object_count: usize,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PartBinding {
    pub part_id: String,
    pub freecad_object_name: String,
    pub label: String,
    pub kind: String,
    #[serde(default)]
    pub semantic_role: Option<String>,
    #[serde(default)]
    pub viewer_asset_path: Option<String>,
    #[serde(default)]
    pub viewer_node_ids: Vec<String>,
    #[serde(default)]
    pub parameter_keys: Vec<String>,
    pub editable: bool,
    #[serde(default)]
    pub bounds: Option<ManifestBounds>,
    #[serde(default)]
    pub volume: Option<f64>,
    #[serde(default)]
    pub area: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ParameterGroup {
    pub group_id: String,
    pub label: String,
    #[serde(default)]
    pub parameter_keys: Vec<String>,
    #[serde(default)]
    pub part_ids: Vec<String>,
    pub editable: bool,
    #[serde(default)]
    pub presentation: Option<String>,
    #[serde(default)]
    pub order: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SelectionTarget {
    pub part_id: String,
    pub viewer_node_id: String,
    pub label: String,
    pub kind: SelectionTargetKind,
    pub editable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EnrichmentProposal {
    pub proposal_id: String,
    pub label: String,
    #[serde(default)]
    pub part_ids: Vec<String>,
    #[serde(default)]
    pub parameter_keys: Vec<String>,
    pub confidence: f32,
    pub status: EnrichmentStatus,
    pub provenance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PrimitiveBinding {
    pub parameter_key: String,
    #[serde(default = "default_primitive_binding_scale")]
    pub scale: f64,
    #[serde(default)]
    pub offset: f64,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ControlPrimitive {
    pub primitive_id: String,
    pub label: String,
    pub kind: ControlPrimitiveKind,
    #[serde(default)]
    pub part_ids: Vec<String>,
    #[serde(default)]
    pub bindings: Vec<PrimitiveBinding>,
    pub editable: bool,
    #[serde(default)]
    pub order: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ControlRelation {
    pub relation_id: String,
    pub source_primitive_id: String,
    pub target_primitive_id: String,
    pub mode: ControlRelationMode,
    #[serde(default = "default_primitive_binding_scale")]
    pub scale: f64,
    #[serde(default)]
    pub offset: f64,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ControlViewSection {
    pub section_id: String,
    pub label: String,
    #[serde(default)]
    pub primitive_ids: Vec<String>,
    #[serde(default)]
    pub collapsed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ControlView {
    pub view_id: String,
    pub label: String,
    pub scope: ControlViewScope,
    #[serde(default)]
    pub part_ids: Vec<String>,
    #[serde(default)]
    pub primitive_ids: Vec<String>,
    #[serde(default)]
    pub sections: Vec<ControlViewSection>,
    #[serde(default, rename = "default")]
    pub is_default: bool,
    pub source: ControlViewSource,
    #[serde(default)]
    pub status: EnrichmentStatus,
    #[serde(default)]
    pub order: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Advisory {
    pub advisory_id: String,
    pub label: String,
    pub severity: AdvisorySeverity,
    #[serde(default)]
    pub primitive_ids: Vec<String>,
    #[serde(default)]
    pub view_ids: Vec<String>,
    pub message: String,
    #[serde(default)]
    pub condition: AdvisoryCondition,
    #[serde(default)]
    pub threshold: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManifestEnrichmentState {
    pub status: EnrichmentStatus,
    #[serde(default)]
    pub proposals: Vec<EnrichmentProposal>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelManifest {
    #[serde(default = "default_model_runtime_schema_version")]
    pub schema_version: u32,
    pub model_id: String,
    pub source_kind: ModelSourceKind,
    pub document: DocumentMetadata,
    #[serde(default)]
    pub parts: Vec<PartBinding>,
    #[serde(default)]
    pub parameter_groups: Vec<ParameterGroup>,
    #[serde(default)]
    pub control_primitives: Vec<ControlPrimitive>,
    #[serde(default)]
    pub control_relations: Vec<ControlRelation>,
    #[serde(default)]
    pub control_views: Vec<ControlView>,
    #[serde(default)]
    pub advisories: Vec<Advisory>,
    #[serde(default)]
    pub selection_targets: Vec<SelectionTarget>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default = "default_manifest_enrichment_state")]
    pub enrichment_state: ManifestEnrichmentState,
}

fn default_model_runtime_schema_version() -> u32 {
    MODEL_RUNTIME_SCHEMA_VERSION
}

fn default_artifact_version() -> u32 {
    1
}

fn default_manifest_enrichment_state() -> ManifestEnrichmentState {
    ManifestEnrichmentState {
        status: EnrichmentStatus::None,
        proposals: Vec::new(),
    }
}

fn default_primitive_binding_scale() -> f64 {
    1.0
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FinalizeStatus {
    Success,
    Error,
    Discarded,
}

impl FinalizeStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Error => "error",
            Self::Discarded => "discarded",
        }
    }
}

impl std::str::FromStr for FinalizeStatus {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "success" => Ok(Self::Success),
            "error" => Ok(Self::Error),
            "discarded" => Ok(Self::Discarded),
            other => Err(AppError::validation(format!(
                "Unknown finalize status '{}'.",
                other
            ))),
        }
    }
}

pub fn validate_ui_spec(ui_spec: &UiSpec) -> AppResult<()> {
    let mut keys = HashSet::new();

    for field in &ui_spec.fields {
        let key = field.key().trim();
        if key.is_empty() {
            return Err(AppError::validation(
                "uiSpec fields must have a non-empty key.",
            ));
        }
        if !keys.insert(key.to_string()) {
            return Err(AppError::validation(format!(
                "uiSpec contains duplicate field key '{}'.",
                key
            )));
        }
        if field.label().trim().is_empty() {
            return Err(AppError::validation(format!(
                "uiSpec field '{}' must have a non-empty label.",
                key
            )));
        }

        match field {
            UiField::Range { min, max, step, .. } | UiField::Number { min, max, step, .. } => {
                if let (Some(min), Some(max)) = (min, max) {
                    if min > max {
                        return Err(AppError::validation(format!(
                            "uiSpec field '{}' has min greater than max.",
                            key
                        )));
                    }
                }
                if let Some(step) = step {
                    if *step <= 0.0 {
                        return Err(AppError::validation(format!(
                            "uiSpec field '{}' must have a positive step value.",
                            key
                        )));
                    }
                }
            }
            UiField::Select { options, .. } => {
                if options.is_empty() {
                    return Err(AppError::validation(format!(
                        "uiSpec select field '{}' must define at least one option.",
                        key
                    )));
                }
            }
            UiField::Checkbox { .. } => {}
        }
    }

    let field_map: HashMap<&str, &UiField> = ui_spec
        .fields
        .iter()
        .map(|field| (field.key(), field))
        .collect();

    for field in &ui_spec.fields {
        match field {
            UiField::Range {
                key,
                min_from,
                max_from,
                ..
            }
            | UiField::Number {
                key,
                min_from,
                max_from,
                ..
            } => {
                for dependency in [min_from.as_deref(), max_from.as_deref()]
                    .into_iter()
                    .flatten()
                {
                    let Some(target) = field_map.get(dependency) else {
                        return Err(AppError::validation(format!(
                            "uiSpec field '{}' references unknown dependency '{}'.",
                            key, dependency
                        )));
                    };
                    if !target.is_numeric() {
                        return Err(AppError::validation(format!(
                            "uiSpec field '{}' can only depend on numeric fields, but '{}' is not numeric.",
                            key, dependency
                        )));
                    }
                }
            }
            UiField::Select { .. } | UiField::Checkbox { .. } => {}
        }
    }

    Ok(())
}

pub fn validate_design_params(params: &DesignParams, ui_spec: &UiSpec) -> AppResult<()> {
    let fields: HashMap<&str, &UiField> = ui_spec
        .fields
        .iter()
        .map(|field| (field.key(), field))
        .collect();

    for field in &ui_spec.fields {
        let Some(value) = params.get(field.key()) else {
            return Err(AppError::validation(format!(
                "initialParams is missing '{}'.",
                field.key()
            )));
        };
        field.validate_value(value)?;
    }

    for key in params.keys() {
        if !fields.contains_key(key.as_str()) {
            return Err(AppError::validation(format!(
                "initialParams contains undeclared key '{}'.",
                key
            )));
        }
    }

    Ok(())
}

pub fn validate_design_output(output: &DesignOutput) -> AppResult<()> {
    validate_ui_spec(&output.ui_spec)?;
    validate_design_params(&output.initial_params, &output.ui_spec)?;
    Ok(())
}

pub fn validate_model_manifest(manifest: &ModelManifest) -> AppResult<()> {
    if manifest.schema_version == 0 {
        return Err(AppError::validation(
            "model manifest schemaVersion must be greater than 0.",
        ));
    }

    if manifest.model_id.trim().is_empty() {
        return Err(AppError::validation(
            "model manifest must include a non-empty modelId.",
        ));
    }

    let mut part_ids = HashSet::new();
    let mut viewer_node_ids = HashSet::new();

    for part in &manifest.parts {
        if part.part_id.trim().is_empty() {
            return Err(AppError::validation(
                "model manifest partIds must be non-empty.",
            ));
        }
        if !part_ids.insert(part.part_id.as_str()) {
            return Err(AppError::validation(format!(
                "model manifest contains duplicate partId '{}'.",
                part.part_id
            )));
        }
        if part.freecad_object_name.trim().is_empty() {
            return Err(AppError::validation(format!(
                "part '{}' is missing freecadObjectName.",
                part.part_id
            )));
        }
        for node_id in &part.viewer_node_ids {
            if node_id.trim().is_empty() {
                return Err(AppError::validation(format!(
                    "part '{}' contains an empty viewer node id.",
                    part.part_id
                )));
            }
            viewer_node_ids.insert(node_id.as_str());
        }
    }

    for group in &manifest.parameter_groups {
        if group.group_id.trim().is_empty() {
            return Err(AppError::validation(
                "model manifest parameterGroups must include non-empty groupId values.",
            ));
        }
        if let Some(presentation) = group.presentation.as_deref() {
            if !matches!(presentation, "primary" | "advanced") {
                return Err(AppError::validation(format!(
                    "parameter group '{}' has unsupported presentation '{}'.",
                    group.group_id, presentation
                )));
            }
        }
        for part_id in &group.part_ids {
            if !part_ids.contains(part_id.as_str()) {
                return Err(AppError::validation(format!(
                    "parameter group '{}' references unknown partId '{}'.",
                    group.group_id, part_id
                )));
            }
        }
    }

    let mut primitive_ids = HashSet::new();
    let mut view_ids = HashSet::new();
    let mut relation_ids = HashSet::new();

    for primitive in &manifest.control_primitives {
        if primitive.primitive_id.trim().is_empty() {
            return Err(AppError::validation(
                "control primitives must include a non-empty primitiveId.",
            ));
        }
        if !primitive_ids.insert(primitive.primitive_id.as_str()) {
            return Err(AppError::validation(format!(
                "control primitive '{}' is duplicated.",
                primitive.primitive_id
            )));
        }
        if primitive.label.trim().is_empty() {
            return Err(AppError::validation(format!(
                "control primitive '{}' must include a non-empty label.",
                primitive.primitive_id
            )));
        }
        if primitive.bindings.is_empty() {
            return Err(AppError::validation(format!(
                "control primitive '{}' must include at least one binding.",
                primitive.primitive_id
            )));
        }
        for part_id in &primitive.part_ids {
            if !part_ids.contains(part_id.as_str()) {
                return Err(AppError::validation(format!(
                    "control primitive '{}' references unknown partId '{}'.",
                    primitive.primitive_id, part_id
                )));
            }
        }
        for binding in &primitive.bindings {
            if binding.parameter_key.trim().is_empty() {
                return Err(AppError::validation(format!(
                    "control primitive '{}' contains a binding with an empty parameterKey.",
                    primitive.primitive_id
                )));
            }
        }
    }

    for view in &manifest.control_views {
        if view.view_id.trim().is_empty() {
            return Err(AppError::validation(
                "control views must include a non-empty viewId.",
            ));
        }
        if !view_ids.insert(view.view_id.as_str()) {
            return Err(AppError::validation(format!(
                "control view '{}' is duplicated.",
                view.view_id
            )));
        }
        if view.label.trim().is_empty() {
            return Err(AppError::validation(format!(
                "control view '{}' must include a non-empty label.",
                view.view_id
            )));
        }
        for part_id in &view.part_ids {
            if !part_ids.contains(part_id.as_str()) {
                return Err(AppError::validation(format!(
                    "control view '{}' references unknown partId '{}'.",
                    view.view_id, part_id
                )));
            }
        }
        for primitive_id in &view.primitive_ids {
            if !primitive_ids.contains(primitive_id.as_str()) {
                return Err(AppError::validation(format!(
                    "control view '{}' references unknown primitiveId '{}'.",
                    view.view_id, primitive_id
                )));
            }
        }
        for section in &view.sections {
            if section.section_id.trim().is_empty() {
                return Err(AppError::validation(format!(
                    "control view '{}' contains a section with an empty sectionId.",
                    view.view_id
                )));
            }
            for primitive_id in &section.primitive_ids {
                if !primitive_ids.contains(primitive_id.as_str()) {
                    return Err(AppError::validation(format!(
                        "control view '{}' section '{}' references unknown primitiveId '{}'.",
                        view.view_id, section.section_id, primitive_id
                    )));
                }
            }
        }
    }

    for relation in &manifest.control_relations {
        if relation.relation_id.trim().is_empty() {
            return Err(AppError::validation(
                "control relations must include a non-empty relationId.",
            ));
        }
        if !relation_ids.insert(relation.relation_id.as_str()) {
            return Err(AppError::validation(format!(
                "control relation '{}' is duplicated.",
                relation.relation_id
            )));
        }
        if !primitive_ids.contains(relation.source_primitive_id.as_str()) {
            return Err(AppError::validation(format!(
                "control relation '{}' references unknown source primitive '{}'.",
                relation.relation_id, relation.source_primitive_id
            )));
        }
        if !primitive_ids.contains(relation.target_primitive_id.as_str()) {
            return Err(AppError::validation(format!(
                "control relation '{}' references unknown target primitive '{}'.",
                relation.relation_id, relation.target_primitive_id
            )));
        }
        if relation.source_primitive_id == relation.target_primitive_id {
            return Err(AppError::validation(format!(
                "control relation '{}' cannot target the same primitive as its source.",
                relation.relation_id
            )));
        }
    }

    for target in &manifest.selection_targets {
        if !part_ids.contains(target.part_id.as_str()) {
            return Err(AppError::validation(format!(
                "selection target '{}' references unknown partId '{}'.",
                target.viewer_node_id, target.part_id
            )));
        }
        if !viewer_node_ids.contains(target.viewer_node_id.as_str()) {
            return Err(AppError::validation(format!(
                "selection target '{}' references an unknown viewer node id.",
                target.viewer_node_id
            )));
        }
    }

    for proposal in &manifest.enrichment_state.proposals {
        if proposal.proposal_id.trim().is_empty() {
            return Err(AppError::validation(
                "enrichment proposals must include a non-empty proposalId.",
            ));
        }
        for part_id in &proposal.part_ids {
            if !part_ids.contains(part_id.as_str()) {
                return Err(AppError::validation(format!(
                    "enrichment proposal '{}' references unknown partId '{}'.",
                    proposal.proposal_id, part_id
                )));
            }
        }
    }

    for advisory in &manifest.advisories {
        if advisory.advisory_id.trim().is_empty() {
            return Err(AppError::validation(
                "advisories must include a non-empty advisoryId.",
            ));
        }
        if advisory.label.trim().is_empty() {
            return Err(AppError::validation(format!(
                "advisory '{}' must include a non-empty label.",
                advisory.advisory_id
            )));
        }
        if advisory.message.trim().is_empty() {
            return Err(AppError::validation(format!(
                "advisory '{}' must include a non-empty message.",
                advisory.advisory_id
            )));
        }
        for primitive_id in &advisory.primitive_ids {
            if !primitive_ids.contains(primitive_id.as_str()) {
                return Err(AppError::validation(format!(
                    "advisory '{}' references unknown primitiveId '{}'.",
                    advisory.advisory_id, primitive_id
                )));
            }
        }
        for view_id in &advisory.view_ids {
            if !view_ids.contains(view_id.as_str()) {
                return Err(AppError::validation(format!(
                    "advisory '{}' references unknown viewId '{}'.",
                    advisory.advisory_id, view_id
                )));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn sample_manifest() -> ModelManifest {
        ModelManifest {
            schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
            model_id: "generated-abc123".to_string(),
            source_kind: ModelSourceKind::Generated,
            document: DocumentMetadata {
                document_name: "Doc".to_string(),
                document_label: "Doc".to_string(),
                source_path: None,
                object_count: 1,
                warnings: Vec::new(),
            },
            parts: vec![PartBinding {
                part_id: "part-shell".to_string(),
                freecad_object_name: "Shell".to_string(),
                label: "Shell".to_string(),
                kind: "Part::Feature".to_string(),
                semantic_role: Some("body".to_string()),
                viewer_asset_path: Some("/tmp/node-shell.stl".to_string()),
                viewer_node_ids: vec!["node-shell".to_string()],
                parameter_keys: vec!["radius".to_string()],
                editable: true,
                bounds: None,
                volume: None,
                area: None,
            }],
            parameter_groups: vec![ParameterGroup {
                group_id: "group-shell".to_string(),
                label: "Shell".to_string(),
                parameter_keys: vec!["radius".to_string()],
                part_ids: vec!["part-shell".to_string()],
                editable: true,
                presentation: Some("primary".to_string()),
                order: Some(0),
            }],
            control_primitives: vec![
                ControlPrimitive {
                    primitive_id: "primitive-shell-radius".to_string(),
                    label: "Shell Radius".to_string(),
                    kind: ControlPrimitiveKind::Number,
                    part_ids: vec!["part-shell".to_string()],
                    bindings: vec![PrimitiveBinding {
                        parameter_key: "radius".to_string(),
                        scale: 1.0,
                        offset: 0.0,
                        min: None,
                        max: None,
                    }],
                    editable: true,
                    order: 0,
                },
                ControlPrimitive {
                    primitive_id: "primitive-shell-radius-target".to_string(),
                    label: "Shell Radius Target".to_string(),
                    kind: ControlPrimitiveKind::Number,
                    part_ids: vec!["part-shell".to_string()],
                    bindings: vec![PrimitiveBinding {
                        parameter_key: "radius".to_string(),
                        scale: 1.0,
                        offset: 0.0,
                        min: None,
                        max: None,
                    }],
                    editable: true,
                    order: 1,
                },
            ],
            control_relations: vec![ControlRelation {
                relation_id: "relation-shell-radius".to_string(),
                source_primitive_id: "primitive-shell-radius".to_string(),
                target_primitive_id: "primitive-shell-radius-target".to_string(),
                mode: ControlRelationMode::Mirror,
                scale: 1.0,
                offset: 0.0,
                enabled: false,
            }],
            control_views: vec![ControlView {
                view_id: "view-shell".to_string(),
                label: "Shell".to_string(),
                scope: ControlViewScope::Part,
                part_ids: vec!["part-shell".to_string()],
                primitive_ids: vec!["primitive-shell-radius".to_string()],
                sections: vec![ControlViewSection {
                    section_id: "section-primary".to_string(),
                    label: "Primary".to_string(),
                    primitive_ids: vec!["primitive-shell-radius".to_string()],
                    collapsed: false,
                }],
                is_default: true,
                source: ControlViewSource::Generated,
                status: EnrichmentStatus::Accepted,
                order: 0,
            }],
            advisories: vec![Advisory {
                advisory_id: "advisory-shell-radius".to_string(),
                label: "Shell note".to_string(),
                severity: AdvisorySeverity::Info,
                primitive_ids: vec!["primitive-shell-radius".to_string()],
                view_ids: vec!["view-shell".to_string()],
                message: "Shell radius drives the body profile.".to_string(),
                condition: AdvisoryCondition::Always,
                threshold: None,
            }],
            selection_targets: vec![SelectionTarget {
                part_id: "part-shell".to_string(),
                viewer_node_id: "node-shell".to_string(),
                label: "Shell".to_string(),
                kind: SelectionTargetKind::Part,
                editable: true,
            }],
            warnings: Vec::new(),
            enrichment_state: ManifestEnrichmentState {
                status: EnrichmentStatus::None,
                proposals: Vec::new(),
            },
        }
    }

    fn sample_manifest_with_shape(
        part_count: usize,
        node_count: usize,
        parameter_count: usize,
    ) -> ModelManifest {
        let parts = (0..part_count)
            .map(|index| {
                let part_id = format!("part-{}", index);
                let viewer_node_ids = (0..node_count)
                    .map(|node_index| format!("node-{}-{}", index, node_index))
                    .collect::<Vec<_>>();
                let parameter_keys = (0..parameter_count)
                    .map(|param_index| format!("param_{}_{}", index, param_index))
                    .collect::<Vec<_>>();
                PartBinding {
                    part_id: part_id.clone(),
                    freecad_object_name: format!("Object{}", index),
                    label: format!("Part {}", index),
                    kind: "Part::Feature".to_string(),
                    semantic_role: Some("unknown".to_string()),
                    viewer_asset_path: Some(format!("/tmp/part-{}.stl", index)),
                    viewer_node_ids,
                    parameter_keys: parameter_keys.clone(),
                    editable: !parameter_keys.is_empty(),
                    bounds: None,
                    volume: None,
                    area: None,
                }
            })
            .collect::<Vec<_>>();

        let selection_targets = parts
            .iter()
            .flat_map(|part| {
                part.viewer_node_ids
                    .iter()
                    .map(|node_id| SelectionTarget {
                        part_id: part.part_id.clone(),
                        viewer_node_id: node_id.clone(),
                        label: part.label.clone(),
                        kind: SelectionTargetKind::Part,
                        editable: part.editable,
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        let parameter_groups = parts
            .iter()
            .filter(|part| !part.parameter_keys.is_empty())
            .map(|part| ParameterGroup {
                group_id: format!("group-{}", part.part_id),
                label: part.label.clone(),
                parameter_keys: part.parameter_keys.clone(),
                part_ids: vec![part.part_id.clone()],
                editable: true,
                presentation: Some("primary".to_string()),
                order: Some(0),
            })
            .collect::<Vec<_>>();

        ModelManifest {
            schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
            model_id: format!(
                "generated-shape-{}-{}-{}",
                part_count, node_count, parameter_count
            ),
            source_kind: ModelSourceKind::Generated,
            document: DocumentMetadata {
                document_name: "Shape".to_string(),
                document_label: "Shape".to_string(),
                source_path: None,
                object_count: part_count,
                warnings: Vec::new(),
            },
            parts,
            parameter_groups,
            control_primitives: Vec::new(),
            control_relations: Vec::new(),
            control_views: Vec::new(),
            advisories: Vec::new(),
            selection_targets,
            warnings: Vec::new(),
            enrichment_state: ManifestEnrichmentState {
                status: EnrichmentStatus::None,
                proposals: Vec::new(),
            },
        }
    }

    #[test]
    fn validate_model_manifest_accepts_consistent_manifest() {
        validate_model_manifest(&sample_manifest()).expect("manifest should be valid");
    }

    #[test]
    fn validate_model_manifest_rejects_unknown_selection_part() {
        let mut manifest = sample_manifest();
        manifest.selection_targets[0].part_id = "missing".to_string();
        let err = validate_model_manifest(&manifest).expect_err("manifest should be invalid");
        assert!(err.message.contains("unknown partId"));
    }

    #[test]
    fn validate_model_manifest_rejects_unknown_relation_target() {
        let mut manifest = sample_manifest();
        manifest.control_relations[0].target_primitive_id = "missing-target".to_string();
        let err = validate_model_manifest(&manifest).expect_err("manifest should be invalid");
        assert!(err.message.contains("unknown target primitive"));
    }

    #[test]
    fn genie_traits_from_seed_stays_within_declared_bounds() {
        for seed in [1u32, 7, 42, 1337, u32::MAX - 4] {
            let traits = GenieTraits::from_seed(seed);
            assert_eq!(traits.version, GENIE_TRAITS_VERSION);
            assert_eq!(traits.seed, seed);
            assert!((0.0..360.0).contains(&traits.color_hue));
            assert!((10..=24).contains(&traits.vertex_count));
            assert!((25.0..=34.0).contains(&traits.radius_base));
            assert!((0.9..=1.06).contains(&traits.stretch_y));
            assert!((0.88..=1.14).contains(&traits.asymmetry));
            assert!((2..=6).contains(&traits.chord_skip));
            assert!((0.7..=1.45).contains(&traits.jitter_scale));
            assert!((0.7..=1.35).contains(&traits.pulse_scale));
            assert!((0.8..=1.6).contains(&traits.hover_scale));
            assert!((0.35..=1.25).contains(&traits.warp_scale));
            assert!((-32.0..=32.0).contains(&traits.glow_hue_shift));
            assert!((15.0..=22.5).contains(&traits.eye_spacing));
            assert!((2.0..=3.6).contains(&traits.eye_size));
            assert!((0.6..=2.6).contains(&traits.mouth_curve));
            assert!((0.2..=1.0).contains(&traits.thinking_bias));
            assert!((0.2..=1.0).contains(&traits.repair_bias));
            assert!((0.2..=1.0).contains(&traits.render_bias));
            assert!((0.35..=1.0).contains(&traits.expressiveness));
        }
    }

    #[test]
    fn decode_genie_traits_json_upgrades_legacy_payload_deterministically() {
        let legacy = serde_json::json!({
            "seed": 4242,
            "colorHue": 122.5,
            "vertexCount": 19,
            "jitterScale": 1.12,
            "pulseScale": 0.84,
        })
        .to_string();

        let first = decode_genie_traits_json(&legacy, Some("thread-alpha"))
            .expect("legacy payload should upgrade");
        let second = decode_genie_traits_json(&legacy, Some("thread-alpha"))
            .expect("legacy payload should upgrade deterministically");

        assert_eq!(first, second);
        assert_eq!(first.version, GENIE_TRAITS_VERSION);
        assert_eq!(first.seed, 4242);
        assert_eq!(first.color_hue, 122.5);
        assert_eq!(first.vertex_count, 19);
        assert_eq!(first.jitter_scale, 1.12);
        assert_eq!(first.pulse_scale, 0.84);
    }

    #[test]
    fn upgraded_or_default_genie_traits_uses_thread_seed_when_traits_are_missing() {
        let thread_id = "thread-without-traits";
        let traits = upgraded_or_default_genie_traits(thread_id, None);

        assert_eq!(traits.version, GENIE_TRAITS_VERSION);
        assert_eq!(traits.seed, derive_thread_seed(thread_id));
    }

    proptest! {
        #[test]
        fn validate_model_manifest_accepts_generated_shapes(
            part_count in 1usize..6,
            node_count in 1usize..4,
            parameter_count in 0usize..4,
        ) {
            let manifest = sample_manifest_with_shape(part_count, node_count, parameter_count);
            prop_assert!(validate_model_manifest(&manifest).is_ok());
        }

        #[test]
        fn validate_model_manifest_rejects_unknown_viewer_nodes(
            part_count in 1usize..6,
            node_count in 1usize..4,
            parameter_count in 0usize..4,
        ) {
            let mut manifest = sample_manifest_with_shape(part_count, node_count, parameter_count);
            manifest.selection_targets[0].viewer_node_id = "missing-node".to_string();

            let err = validate_model_manifest(&manifest).expect_err("manifest should reject unknown viewer nodes");
            prop_assert!(err.message.contains("unknown viewer node id"));
        }
    }
}
