use rusqlite::types::{FromSql, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum EngineKind {
    Freecad,
    #[serde(rename = "ecky", alias = "eckyIrV0", alias = "ecky_ir_v0")]
    #[specta(rename = "ecky")]
    #[default]
    EckyIrV0,
    #[serde(rename = "build123d")]
    #[specta(rename = "build123d")]
    Build123d,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum SourceLanguage {
    LegacyPython,
    #[serde(rename = "ecky", alias = "eckyIrV0", alias = "ecky_ir_v0")]
    #[specta(rename = "ecky")]
    #[default]
    EckyIrV0,
    #[serde(rename = "build123d")]
    #[specta(rename = "build123d")]
    Build123d,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq, Default)]
pub enum GeometryBackend {
    #[serde(rename = "freecad")]
    Freecad,
    #[default]
    #[serde(rename = "build123d")]
    Build123d,
    #[serde(
        rename = "mesh",
        alias = "native",
        alias = "eckyRust",
        alias = "ecky_rust"
    )]
    #[specta(rename = "mesh")]
    EckyRust,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MacroDialect {
    Legacy,
    CadFrameworkV1,
    #[serde(rename = "ecky", alias = "eckyIrV0", alias = "ecky_ir_v0")]
    #[specta(rename = "ecky")]
    EckyIrV0,
    #[serde(rename = "build123d")]
    #[specta(rename = "build123d")]
    Build123d,
}

impl EngineKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Freecad => "freecad",
            Self::EckyIrV0 => "ecky",
            Self::Build123d => "build123d",
        }
    }

    pub fn to_source_language(&self) -> SourceLanguage {
        match self {
            Self::Freecad => SourceLanguage::LegacyPython,
            Self::EckyIrV0 => SourceLanguage::EckyIrV0,
            Self::Build123d => SourceLanguage::Build123d,
        }
    }

    pub fn to_geometry_backend(&self) -> GeometryBackend {
        match self {
            Self::Freecad => GeometryBackend::Freecad,
            Self::EckyIrV0 => GeometryBackend::EckyRust,
            Self::Build123d => GeometryBackend::Build123d,
        }
    }
}

impl std::str::FromStr for EngineKind {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "freecad" => Ok(Self::Freecad),
            "ecky" | "eckyIrV0" | "ecky_ir_v0" => Ok(Self::EckyIrV0),
            "build123d" => Ok(Self::Build123d),
            _ => Err(()),
        }
    }
}

impl SourceLanguage {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LegacyPython => "legacyPython",
            Self::EckyIrV0 => "ecky",
            Self::Build123d => "build123d",
        }
    }

    pub fn to_engine_kind(&self) -> EngineKind {
        match self {
            Self::LegacyPython => EngineKind::Freecad,
            Self::EckyIrV0 => EngineKind::EckyIrV0,
            Self::Build123d => EngineKind::Build123d,
        }
    }
}

impl std::str::FromStr for SourceLanguage {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "legacyPython" | "legacy_python" => Ok(Self::LegacyPython),
            "ecky" | "eckyIrV0" | "ecky_ir_v0" => Ok(Self::EckyIrV0),
            "build123d" => Ok(Self::Build123d),
            _ => Err(()),
        }
    }
}

impl GeometryBackend {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Freecad => "freecad",
            Self::Build123d => "build123d",
            Self::EckyRust => "mesh",
        }
    }
}

impl std::str::FromStr for GeometryBackend {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "freecad" => Ok(Self::Freecad),
            "build123d" => Ok(Self::Build123d),
            "mesh" | "native" | "eckyRust" | "ecky_rust" => Ok(Self::EckyRust),
            _ => Err(()),
        }
    }
}

impl MacroDialect {
    pub fn is_framework(&self) -> bool {
        matches!(self, Self::CadFrameworkV1)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeBackendCapability {
    pub available: bool,
    pub detail: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeAuthoringContext {
    pub engine_kind: EngineKind,
    pub source_language: SourceLanguage,
    pub geometry_backend: GeometryBackend,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCapabilities {
    pub freecad: RuntimeBackendCapability,
    pub build123d: RuntimeBackendCapability,
    pub direct_occt: RuntimeBackendCapability,
    #[serde(rename = "mesh", alias = "eckyRust")]
    #[specta(rename = "mesh")]
    pub ecky_rust: RuntimeBackendCapability,
    pub recommended_authoring_context: RuntimeAuthoringContext,
}

impl ToSql for EngineKind {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_str()))
    }
}

impl FromSql for EngineKind {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let raw = value.as_str()?;
        Ok(raw.parse().unwrap_or_default())
    }
}

impl ToSql for SourceLanguage {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_str()))
    }
}

impl FromSql for SourceLanguage {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let raw = value.as_str()?;
        Ok(raw.parse().unwrap_or_default())
    }
}

impl ToSql for GeometryBackend {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_str()))
    }
}

impl FromSql for GeometryBackend {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let raw = value.as_str()?;
        Ok(raw.parse().unwrap_or_default())
    }
}
