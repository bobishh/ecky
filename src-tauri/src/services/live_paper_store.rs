//! Durable accepted draft; video frames and pairing credentials remain transient.
use super::live_paper::LivePaperPrimitive;
use crate::contracts::SketchView;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PaperDraft {
    pub scope_key: String,
    pub view: SketchView,
    pub accepted: Vec<LivePaperPrimitive>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub other_views: Vec<PaperViewDraft>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PaperViewDraft {
    pub view: SketchView,
    pub accepted: Vec<LivePaperPrimitive>,
}

const MAX_DRAFT_BYTES: u64 = 16 * 1024 * 1024;

pub fn load(path: &Path) -> Result<Option<PaperDraft>, String> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Could not inspect accepted paper draft: {error}")),
    };
    if metadata.len() > MAX_DRAFT_BYTES {
        return Err("Accepted paper draft exceeds the 16 MiB limit.".into());
    }
    let bytes = std::fs::read(path)
        .map_err(|error| format!("Could not read accepted paper draft: {error}"))?;
    let draft = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Could not restore accepted paper draft: {error}"))?;
    Ok(Some(draft))
}

pub fn save(path: &Path, draft: &PaperDraft) -> Result<(), String> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_WRITE: AtomicU64 = AtomicU64::new(0);
    let bytes = serde_json::to_vec(draft)
        .map_err(|error| format!("Could not encode accepted paper draft: {error}"))?;
    if bytes.len() as u64 > MAX_DRAFT_BYTES {
        return Err("Accepted paper draft exceeds the 16 MiB limit.".into());
    }
    let parent = path
        .parent()
        .ok_or("Accepted paper draft needs a parent directory.")?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create paper draft directory: {error}"))?;
    let temporary = path.with_extension(format!(
        "{}.{}.tmp",
        std::process::id(),
        NEXT_WRITE.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)
    })()
    .map_err(|error: std::io::Error| format!("Could not save accepted paper draft: {error}"));
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn given_accepted_draft_when_saved_and_reopened_then_keep_identity_without_video_or_credentials(
    ) {
        let directory =
            std::env::temp_dir().join(format!("ecky-paper-store-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("draft.json");
        let draft = PaperDraft {
            other_views: Vec::new(),
            scope_key: "model-head".into(),
            view: SketchView::Front,
            accepted: vec![LivePaperPrimitive::Point {
                id: "kept".into(),
                position_mm: [20.0, 30.0],
                residual_mm: 0.1,
            }],
        };
        save(&path, &draft).unwrap();
        assert_eq!(load(&path).unwrap(), Some(draft));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("token") && !text.contains("preview"));
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn given_corrupt_draft_when_loaded_then_surface_error_and_preserve_file() {
        let path =
            std::env::temp_dir().join(format!("ecky-paper-corrupt-{}.json", std::process::id()));
        std::fs::write(&path, "not valid JSON").unwrap();
        assert!(load(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "not valid JSON");
        std::fs::remove_file(path).unwrap();
    }
}
