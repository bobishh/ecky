use super::{AppError, AppResult};
pub use cairn_registry::{
    PackageDependency as CairnDependency, PackageDistribution as CairnDistribution,
};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Clone, Debug, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CairnLockedRelease {
    pub package_id: String,
    pub version: String,
    pub package_digest: String,
    pub archive_digest: String,
    #[serde(default)]
    pub dependencies: Vec<CairnDependency>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CairnConfig {
    pub registry_url: String,
}

/// Exact install environment inside the existing canonical model dependency lock.
#[derive(Clone, Debug, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CairnLock {
    pub registry: String,
    pub roots: Vec<CairnDependency>,
    pub releases: Vec<CairnLockedRelease>,
}

pub fn validate_cairn_dependencies(dependencies: &[CairnDependency]) -> AppResult<()> {
    let mut ids = std::collections::BTreeSet::new();
    if dependencies.len() > 64 {
        return Err(AppError::validation(
            "At most 64 direct Cairn dependencies are supported",
        ));
    }
    for dependency in dependencies {
        if !cairn_registry::valid_segment(&dependency.package_id)
            || !cairn_registry::valid_segment(&dependency.version)
            || !ids.insert(&dependency.package_id)
        {
            return Err(AppError::validation(
                "Cairn dependencies require unique package IDs and exact versions",
            ));
        }
    }
    Ok(())
}
pub fn validate_cairn_registry(registry: &str) -> AppResult<()> {
    let url =
        reqwest::Url::parse(registry).map_err(|error| AppError::validation(error.to_string()))?;
    let local = matches!(
        url.host_str(),
        Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
    );
    if url.host_str().is_none()
        || (url.scheme() != "https" && !(url.scheme() == "http" && local))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(AppError::validation("Cairn registry must use HTTPS (local HTTP allowed) without credentials, query or fragment"));
    }
    Ok(())
}
impl CairnLock {
    pub fn canonical(mut self) -> Self {
        self.registry = self.registry.trim_end_matches('/').to_string();
        self.roots.sort_by(|a, b| {
            a.package_id
                .cmp(&b.package_id)
                .then(a.version.cmp(&b.version))
        });
        self.releases.sort_by(|a, b| {
            a.package_id
                .cmp(&b.package_id)
                .then(a.version.cmp(&b.version))
        });
        for release in &mut self.releases {
            release.dependencies.sort_by(|a, b| {
                a.package_id
                    .cmp(&b.package_id)
                    .then(a.version.cmp(&b.version))
            });
        }
        self
    }
    pub fn validate(&self) -> AppResult<()> {
        use std::collections::{BTreeMap, BTreeSet};
        validate_cairn_registry(&self.registry)?;
        validate_cairn_dependencies(&self.roots)?;
        if self.releases.len() > 128 {
            return Err(AppError::validation("Cairn graph exceeds 128 releases"));
        }
        let mut by_id = BTreeMap::new();
        for release in &self.releases {
            if !cairn_registry::valid_segment(&release.package_id)
                || !cairn_registry::valid_segment(&release.version)
                || !cairn_registry::valid_digest(&release.archive_digest)
                || !cairn_registry::valid_digest(&release.package_digest)
                || by_id.insert(release.package_id.as_str(), release).is_some()
            {
                return Err(AppError::validation(
                    "Cairn lock contains invalid or conflicting immutable releases",
                ));
            }
            validate_cairn_dependencies(&release.dependencies)?;
        }
        fn visit<'a>(
            dependency: &CairnDependency,
            graph: &BTreeMap<&'a str, &'a CairnLockedRelease>,
            active: &mut BTreeSet<String>,
            heights: &mut BTreeMap<String, usize>,
            depth: usize,
        ) -> AppResult<usize> {
            if depth > 16 {
                return Err(AppError::validation("Cairn graph exceeds depth 16"));
            }
            let release = graph.get(dependency.package_id.as_str()).ok_or_else(|| {
                AppError::validation(format!(
                    "Missing locked release {}@{}",
                    dependency.package_id, dependency.version
                ))
            })?;
            if release.version != dependency.version {
                return Err(AppError::validation(
                    "Conflicting exact versions in Cairn graph",
                ));
            }
            if let Some(height) = heights.get(&dependency.package_id).copied() {
                if depth + height > 16 {
                    return Err(AppError::validation("Cairn graph exceeds depth 16"));
                }
                return Ok(height);
            }
            if !active.insert(dependency.package_id.clone()) {
                return Err(AppError::validation(
                    "Cyclic Cairn dependencies are unsupported",
                ));
            }
            let mut height = 0;
            for child in &release.dependencies {
                height = height.max(1 + visit(child, graph, active, heights, depth + 1)?);
            }
            active.remove(&dependency.package_id);
            heights.insert(dependency.package_id.clone(), height);
            Ok(height)
        }
        let mut seen = BTreeMap::new();
        for root in &self.roots {
            visit(root, &by_id, &mut BTreeSet::new(), &mut seen, 0)?;
        }
        if seen.len() != self.releases.len() {
            return Err(AppError::validation(
                "Cairn lock contains releases unreachable from declared roots",
            ));
        }
        Ok(())
    }
}
