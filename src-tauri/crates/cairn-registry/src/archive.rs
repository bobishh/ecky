use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
};

pub const MAX_RELEASE_METADATA_BYTES: usize = 64 * 1024;
pub const MAX_ARCHIVE_BYTES: usize = 32 * 1024 * 1024;
const MAX_EXPANDED_BYTES: u64 = 128 * 1024 * 1024;
const MAX_ENTRIES: usize = 4096;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PackageDependency {
    pub package_id: String,
    pub version: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PackageDistribution {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(default)]
    pub dependencies: Vec<PackageDependency>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub package_id: String,
    pub version: String,
    pub display_name: String,
    pub archive_digest: String,
    pub package_digest: String,
    pub size: u64,
    pub yanked: bool,
    pub distribution: PackageDistribution,
    pub header: Value,
}

pub fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}
pub fn valid_segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        && value != "."
        && value != ".."
}
pub fn valid_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

fn entries(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    if zip.len() > MAX_ENTRIES {
        return Err("Archive contains too many entries".into());
    }
    let mut expanded = 0u64;
    let mut result = BTreeMap::new();
    for i in 0..zip.len() {
        let file = zip.by_index(i).map_err(|e| e.to_string())?;
        let path = std::str::from_utf8(file.name_raw())
            .map_err(|_| "Archive paths must be UTF-8")?
            .to_string();
        if path.contains(['\\', '\0', ':'])
            || path.starts_with('/')
            || path.split('/').any(|p| matches!(p, "." | ".."))
        {
            return Err(format!("Unsafe archive path: {path}"));
        }
        let mode = file.unix_mode().unwrap_or(0) & 0o170000;
        if !matches!(mode, 0 | 0o100000 | 0o040000) {
            return Err(format!("Archive entry is not a regular file: {path}"));
        }
        if file.is_dir() {
            continue;
        }
        if path.split('/').any(str::is_empty) {
            return Err(format!("Noncanonical archive path: {path}"));
        }
        expanded = expanded
            .checked_add(file.size())
            .ok_or("Expanded archive size overflow")?;
        if expanded > MAX_EXPANDED_BYTES {
            return Err("Expanded archive exceeds 128 MiB".into());
        }
        let mut body = Vec::new();
        file.take(MAX_EXPANDED_BYTES + 1)
            .read_to_end(&mut body)
            .map_err(|e| e.to_string())?;
        if body.len() as u64 > MAX_EXPANDED_BYTES {
            return Err("Expanded entry exceeds limit".into());
        }
        if result.insert(path.clone(), body).is_some() {
            return Err(format!("Duplicate archive path: {path}"));
        }
    }
    Ok(result)
}

/// Validate the portable envelope and compute the same length-delimited payload identity as Ecky's store.
/// Desktop clients additionally validate CAD contracts before installation.
pub fn inspect_archive(bytes: &[u8]) -> Result<Release, String> {
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err("Archive exceeds 32 MiB".into());
    }
    let envelope = entries(bytes)?;
    if envelope.len() != 2 {
        return Err("Expected ecky-header.json and ecky-payload.b64 envelope".into());
    }
    let header: Value = serde_json::from_slice(
        envelope
            .get("ecky-header.json")
            .ok_or("Missing package header")?,
    )
    .map_err(|e| e.to_string())?;
    let payload = base64::engine::general_purpose::STANDARD
        .decode(
            envelope
                .get("ecky-payload.b64")
                .ok_or("Missing package payload")?,
        )
        .map_err(|e| e.to_string())?;
    let files = entries(&payload)?;
    let manifest: Value = serde_json::from_slice(
        files
            .get("ecky-package.json")
            .ok_or("Missing package manifest")?,
    )
    .map_err(|e| e.to_string())?;
    if manifest["schemaVersion"] != 1 || header["schemaVersion"] != 1 {
        return Err("Unsupported package schemaVersion".into());
    }
    let field = |key: &str| {
        manifest[key]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| format!("Missing {key}"))
    };
    let package_id = field("packageId")?;
    let version = field("version")?;
    if !valid_segment(&package_id) || !valid_segment(&version) {
        return Err("Invalid package coordinate".into());
    }
    if header["packageId"] != package_id || header["version"] != version {
        return Err("Header/payload coordinate mismatch".into());
    }
    let distribution: PackageDistribution = manifest
        .get("cairn")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    if distribution.dependencies.len() > 64 {
        return Err("At most 64 direct dependencies are supported".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    for dependency in &distribution.dependencies {
        if !valid_segment(&dependency.package_id)
            || !valid_segment(&dependency.version)
            || !seen.insert(&dependency.package_id)
        {
            return Err("Dependencies require unique package IDs and exact versions".into());
        }
    }
    let components = manifest["components"]
        .as_array()
        .ok_or("Package components must be an array")?;
    for component in components {
        let id = component["componentId"]
            .as_str()
            .ok_or("Component ID is required")?;
        if !valid_segment(id) {
            return Err("Invalid component ID".into());
        }
        if let Some(path) = component["sourceRef"].as_str() {
            if !files.contains_key(path) {
                return Err(format!("Missing sourceRef: {path}"));
            }
        }
    }
    let mut hasher = Sha256::new();
    hasher.update(b"ecky-package-payload-v1\0");
    for (path, body) in &files {
        if matches!(path.as_str(), "ecky-header.json" | "ecky-payload.b64") {
            continue;
        }
        if path == "ecky-integrity.json" {
            return Err("Runtime integrity sidecar is reserved".into());
        }
        hasher.update((path.len() as u64).to_be_bytes());
        hasher.update(path.as_bytes());
        hasher.update((body.len() as u64).to_be_bytes());
        hasher.update(body);
    }
    let release = Release {
        package_id,
        version,
        display_name: field("displayName")?,
        archive_digest: sha256(bytes),
        package_digest: format!("sha256:{:x}", hasher.finalize()),
        size: bytes.len() as u64,
        yanked: false,
        distribution,
        header,
    };
    if serde_json::to_vec(&release)
        .map_err(|error| error.to_string())?
        .len()
        > MAX_RELEASE_METADATA_BYTES
    {
        return Err(
            "Release metadata exceeds 64 KiB; split the package or reduce its public header".into(),
        );
    }
    Ok(release)
}

/// Read one intentionally published text export; large/binary assets stay downloadable as archives.
pub(crate) fn component_source(bytes: &[u8], component_id: &str) -> Result<Option<String>, String> {
    let envelope = entries(bytes)?;
    let payload = base64::engine::general_purpose::STANDARD
        .decode(
            envelope
                .get("ecky-payload.b64")
                .ok_or("Missing package payload")?,
        )
        .map_err(|error| error.to_string())?;
    let mut zip = zip::ZipArchive::new(Cursor::new(payload)).map_err(|error| error.to_string())?;
    let manifest: Value = {
        let file = zip
            .by_name("ecky-package.json")
            .map_err(|error| error.to_string())?;
        if file.size() > 2 * 1024 * 1024 {
            return Err("Manifest exceeds inspector limit".into());
        }
        serde_json::from_reader(file).map_err(|error| error.to_string())?
    };
    let export = manifest["components"]
        .as_array()
        .and_then(|components| {
            components
                .iter()
                .find(|component| component["componentId"] == component_id)
        })
        .ok_or("Component is not exported by this release")?;
    let Some(path) = export["sourceRef"].as_str() else {
        return Ok(None);
    };
    if ![".ecky", ".scm", ".py"]
        .iter()
        .any(|extension| path.ends_with(extension))
    {
        return Ok(None);
    }
    let mut file = zip.by_name(path).map_err(|error| error.to_string())?;
    if file.size() > 512 * 1024 {
        return Ok(None);
    }
    let mut source = String::new();
    file.read_to_string(&mut source)
        .map_err(|error| error.to_string())?;
    Ok(Some(source))
}
