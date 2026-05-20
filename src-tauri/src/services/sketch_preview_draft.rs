use crate::models::{
    AppError, AppResult, ArtifactBundle, ClearSketchPreviewDraftRequest,
    LoadSketchPreviewDraftRequest, PathResolver, SaveSketchPreviewDraftRequest, SketchDraftSource,
    SketchPreviewDraft,
};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const SKETCH_PREVIEW_DRAFT_DIR: &str = "sketch_preview_drafts";
const GLOBAL_SCOPE_ID: &str = "global";

fn scope_key(scope_id: Option<&str>) -> String {
    let scope_id = scope_id.unwrap_or(GLOBAL_SCOPE_ID).trim();
    if scope_id.is_empty() {
        GLOBAL_SCOPE_ID.to_string()
    } else {
        scope_id.replace('/', "_")
    }
}

pub fn sketch_preview_draft_path(app: &dyn PathResolver, scope_id: Option<&str>) -> PathBuf {
    app.app_config_dir()
        .join(SKETCH_PREVIEW_DRAFT_DIR)
        .join(format!("{}.json", scope_key(scope_id)))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn persist_sketch_preview_draft(
    path: &PathBuf,
    scope_id: Option<&str>,
    draft_source: SketchDraftSource,
    artifact_bundle: ArtifactBundle,
) -> AppResult<SketchPreviewDraft> {
    let draft = SketchPreviewDraft {
        scope_id: scope_id
            .map(|value| value.to_string())
            .or(Some(GLOBAL_SCOPE_ID.to_string())),
        draft_source,
        artifact_bundle,
        updated_at: now_secs(),
    };
    let serialized = serde_json::to_string_pretty(&draft).map_err(|e| {
        AppError::persistence(format!("Failed to serialize sketch preview draft: {e}"))
    })?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            AppError::persistence(format!(
                "Failed to create sketch preview draft directory {}: {e}",
                parent.display()
            ))
        })?;
    }
    fs::write(path, serialized).map_err(|e| {
        AppError::persistence(format!(
            "Failed to write sketch preview draft at {}: {e}",
            path.display()
        ))
    })?;
    Ok(draft)
}

fn write_serialized_draft(path: &PathBuf, serialized: &str) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            AppError::persistence(format!(
                "Failed to create sketch preview draft directory {}: {e}",
                parent.display()
            ))
        })?;
    }
    fs::write(path, serialized).map_err(|e| {
        AppError::persistence(format!(
            "Failed to write sketch preview draft at {}: {e}",
            path.display()
        ))
    })
}

pub fn save_sketch_preview_draft(
    app: &dyn PathResolver,
    request: SaveSketchPreviewDraftRequest,
) -> AppResult<SketchPreviewDraft> {
    let path = sketch_preview_draft_path(app, request.scope_id.as_deref());
    let request_scope = scope_key(request.scope_id.as_deref());
    let draft = persist_sketch_preview_draft(
        &path,
        request.scope_id.as_deref(),
        request.draft_source,
        request.artifact_bundle,
    )?;
    if request_scope != GLOBAL_SCOPE_ID {
        let global_path = sketch_preview_draft_path(app, Some(GLOBAL_SCOPE_ID));
        let serialized = serde_json::to_string_pretty(&draft).map_err(|e| {
            AppError::persistence(format!("Failed to serialize sketch preview draft: {e}"))
        })?;
        write_serialized_draft(&global_path, &serialized)?;
    }
    Ok(draft)
}

pub fn load_sketch_preview_draft(
    app: &dyn PathResolver,
    request: LoadSketchPreviewDraftRequest,
) -> AppResult<Option<SketchPreviewDraft>> {
    let path = sketch_preview_draft_path(app, request.scope_id.as_deref());
    let Ok(serialized) = fs::read_to_string(&path) else {
        return match path.exists() {
            false => Ok(None),
            true => Err(AppError::persistence(format!(
                "Failed to read sketch preview draft at {}.",
                path.display()
            ))),
        };
    };
    let draft = serde_json::from_str::<SketchPreviewDraft>(&serialized).map_err(|e| {
        AppError::persistence(format!(
            "Failed to parse sketch preview draft at {}: {e}",
            path.display()
        ))
    })?;
    Ok(Some(draft))
}

pub fn clear_sketch_preview_draft(
    app: &dyn PathResolver,
    request: ClearSketchPreviewDraftRequest,
) -> AppResult<()> {
    let primary_path = sketch_preview_draft_path(app, request.scope_id.as_deref());
    let global_path = sketch_preview_draft_path(app, Some(GLOBAL_SCOPE_ID));
    for path in [primary_path, global_path] {
        match fs::remove_file(&path) {
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(AppError::persistence(format!(
                    "Failed to clear sketch preview draft at {}: {err}",
                    path.display()
                )))
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        ArtifactBundle, EngineKind, GeometryBackend, MacroDialect, ModelSourceKind, SourceLanguage,
    };
    use std::path::Path;

    struct TestResolver {
        config_dir: PathBuf,
    }

    impl PathResolver for TestResolver {
        fn app_config_dir(&self) -> PathBuf {
            self.config_dir.clone()
        }

        fn app_data_dir(&self) -> PathBuf {
            self.config_dir.clone()
        }

        fn resource_path(&self, _path: &str) -> Option<PathBuf> {
            None
        }
    }

    fn test_artifact_bundle() -> ArtifactBundle {
        ArtifactBundle {
            schema_version: 1,
            model_id: "model-1".to_string(),
            source_kind: ModelSourceKind::Generated,
            engine_kind: EngineKind::EckyIrV0,
            source_language: SourceLanguage::EckyIrV0,
            geometry_backend: GeometryBackend::EckyRust,
            content_hash: "hash".to_string(),
            artifact_version: 1,
            fcstd_path: "/tmp/model.fcstd".to_string(),
            manifest_path: "/tmp/model.json".to_string(),
            macro_path: Some("/tmp/model.ecky".to_string()),
            preview_stl_path: "/tmp/model.stl".to_string(),
            viewer_assets: Vec::new(),
            edge_targets: Vec::new(),
            face_targets: Vec::new(),
            callout_anchors: Vec::new(),
            measurement_guides: Vec::new(),
            export_artifacts: Vec::new(),
        }
    }

    fn test_draft_source() -> SketchDraftSource {
        SketchDraftSource {
            source_language: SourceLanguage::EckyIrV0,
            geometry_backend: GeometryBackend::EckyRust,
            macro_dialect: MacroDialect::EckyIrV0,
            source: "(model)".to_string(),
            warnings: vec![],
        }
    }

    fn test_resolver(root: &Path) -> TestResolver {
        TestResolver {
            config_dir: root.to_path_buf(),
        }
    }

    #[test]
    fn sketch_preview_draft_save_load_clear_roundtrip() {
        let root = std::env::temp_dir().join(format!("ecky-sketch-draft-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let resolver = test_resolver(&root);
        let request = SaveSketchPreviewDraftRequest {
            scope_id: Some("thread-a".to_string()),
            draft_source: test_draft_source(),
            artifact_bundle: test_artifact_bundle(),
        };

        let saved = save_sketch_preview_draft(&resolver, request).unwrap();
        assert!(saved.updated_at > 0);
        assert_eq!(saved.draft_source.source, "(model)");
        assert_eq!(saved.scope_id.as_deref(), Some("thread-a"));
        assert_eq!(
            sketch_preview_draft_path(&resolver, Some("thread-a")),
            root.join(SKETCH_PREVIEW_DRAFT_DIR).join("thread-a.json")
        );
        let loaded = load_sketch_preview_draft(
            &resolver,
            LoadSketchPreviewDraftRequest {
                scope_id: Some("thread-a".to_string()),
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(loaded, saved);

        clear_sketch_preview_draft(
            &resolver,
            ClearSketchPreviewDraftRequest {
                scope_id: Some("thread-a".to_string()),
            },
        )
        .unwrap();
        assert!(load_sketch_preview_draft(
            &resolver,
            LoadSketchPreviewDraftRequest {
                scope_id: Some("thread-a".to_string()),
            },
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn sketch_preview_draft_load_missing_returns_none() {
        let root = std::env::temp_dir().join(format!(
            "ecky-sketch-draft-missing-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let resolver = test_resolver(&root);
        assert!(load_sketch_preview_draft(
            &resolver,
            LoadSketchPreviewDraftRequest { scope_id: None },
        )
        .unwrap()
        .is_none());
    }
}
