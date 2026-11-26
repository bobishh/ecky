use serde::de::DeserializeOwned;
use std::fs;
use std::path::{Path, PathBuf};

use crate::contracts::{AppError, AppResult};

pub(crate) const TOPOLOGY_FILE_NAME: &str = "topology.msgpack";
pub(crate) const ANALYSIS_BOUNDARY_FILE_NAME: &str = "analysis-boundary.msgpack";

pub(crate) fn topology_path(directory: &Path) -> PathBuf {
    prefer_binary(directory, TOPOLOGY_FILE_NAME, "topology.json")
}

pub(crate) fn analysis_boundary_path(directory: &Path) -> PathBuf {
    let dedicated = prefer_binary(
        directory,
        ANALYSIS_BOUNDARY_FILE_NAME,
        "analysis-boundary.json",
    );
    if dedicated.is_file() {
        dedicated
    } else {
        topology_path(directory)
    }
}

fn prefer_binary(directory: &Path, binary_name: &str, legacy_name: &str) -> PathBuf {
    let binary = directory.join(binary_name);
    if binary.is_file() {
        binary
    } else {
        directory.join(legacy_name)
    }
}

pub(crate) fn read<T: DeserializeOwned>(path: &Path) -> AppResult<T> {
    let data = fs::read(path).map_err(|error| {
        AppError::persistence(format!(
            "Cannot read Direct OCCT topology '{}': {error}",
            path.display()
        ))
    })?;
    let decoded = if path
        .extension()
        .is_some_and(|extension| extension == "msgpack")
    {
        rmp_serde::from_slice(&data).map_err(|error| error.to_string())
    } else {
        serde_json::from_slice(&data).map_err(|error| error.to_string())
    };
    decoded.map_err(|error| {
        AppError::validation(format!(
            "Invalid Direct OCCT topology '{}': {error}",
            path.display()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_both_encodings_when_loading_topology_then_binary_wins() {
        let dir = std::env::temp_dir().join(format!("ecky-topology-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let legacy = dir.join("topology.json");
        let value = serde_json::json!({"parts": [{"partId": "board"}]});
        fs::write(&legacy, serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(
            read::<serde_json::Value>(&topology_path(&dir)).unwrap(),
            value
        );

        let binary = dir.join(TOPOLOGY_FILE_NAME);
        fs::write(&binary, rmp_serde::to_vec_named(&value).unwrap()).unwrap();
        fs::write(&legacy, b"obsolete").unwrap();
        assert_eq!(
            read::<serde_json::Value>(&topology_path(&dir)).unwrap(),
            value
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
