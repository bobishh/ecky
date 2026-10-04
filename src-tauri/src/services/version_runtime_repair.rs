use crate::contracts::{
    validate_model_runtime_bundle, AppError, AppResult, ArtifactBundle, DesignOutput,
    ModelManifest, WorkspaceProjection,
};
use crate::models::{AppState, PathResolver};
use crate::services::render_snapshot::{
    build_render_snapshot, canonical_version_input_digest, RenderSnapshotInput,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::future::Future;
use std::pin::Pin;

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepairVersionRuntimeIntent {
    pub thread_id: String,
    pub message_id: String,
    #[serde(default)]
    pub expected_artifact_identity: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RepairVersionRuntimeResponse {
    pub snapshot_id: String,
    pub artifact_identity: String,
    pub workspace: WorkspaceProjection,
}

#[derive(Clone)]
struct RepairCandidate {
    design: DesignOutput,
    previous_manifest: Option<ModelManifest>,
    version_input_identity: String,
    artifact_identity: Option<String>,
}

type RenderFuture<'a> =
    Pin<Box<dyn Future<Output = AppResult<(ArtifactBundle, ModelManifest)>> + Send + 'a>>;

trait VersionRuntimeRenderer: Send + Sync {
    fn render<'a>(
        &'a self,
        design: &'a DesignOutput,
        previous_manifest: Option<&'a ModelManifest>,
        state: &'a AppState,
        app: &'a dyn PathResolver,
    ) -> RenderFuture<'a>;
}

struct CanonicalVersionRuntimeRenderer;

impl VersionRuntimeRenderer for CanonicalVersionRuntimeRenderer {
    fn render<'a>(
        &'a self,
        design: &'a DesignOutput,
        previous_manifest: Option<&'a ModelManifest>,
        state: &'a AppState,
        app: &'a dyn PathResolver,
    ) -> RenderFuture<'a> {
        Box::pin(async move {
            let artifact_bundle = crate::services::render::render_model_with_previous_manifest(
                &design.macro_code,
                &design.initial_params,
                Some(design.macro_dialect.clone()),
                Some(design.geometry_backend),
                design.post_processing.as_ref(),
                previous_manifest,
                state,
                app,
            )
            .await?;
            let model_manifest =
                crate::model_runtime::read_model_manifest(app, &artifact_bundle.model_id)?;
            Ok((artifact_bundle, model_manifest))
        })
    }
}

fn candidate_artifact_identity(
    artifact_bundle: Option<&ArtifactBundle>,
) -> AppResult<Option<String>> {
    artifact_bundle
        .map(|bundle| {
            let identity = bundle.content_hash.trim();
            if identity.is_empty() {
                return Err(AppError::validation(
                    "Stored artifact contentHash is empty; runtime repair identity is unavailable.",
                ));
            }
            Ok(identity.to_string())
        })
        .transpose()
}

fn validate_expected_artifact_identity(
    expected: Option<&str>,
    current: Option<&str>,
) -> AppResult<()> {
    let Some(expected) = expected else {
        return Ok(());
    };
    if current == Some(expected) {
        return Ok(());
    }
    Err(AppError::conflict(format!(
        "Version artifact identity changed before runtime repair: expected '{}', current '{}'.",
        expected,
        current.unwrap_or("none")
    )))
}

fn load_candidate(
    conn: &rusqlite::Connection,
    intent: &RepairVersionRuntimeIntent,
) -> AppResult<RepairCandidate> {
    let message =
        crate::db::get_thread_message_version(conn, &intent.thread_id, &intent.message_id)
            .map_err(|error| AppError::persistence(error.to_string()))?
            .ok_or_else(|| AppError::not_found("Version not found for runtime repair."))?;
    let latest = crate::db::get_thread_latest_version(conn, &intent.thread_id)
        .map_err(|error| AppError::persistence(error.to_string()))?
        .ok_or_else(|| AppError::not_found("Thread has no version to repair."))?;
    if latest.id != intent.message_id {
        return Err(AppError::conflict(
            "Only the current thread head may retain repaired runtime.",
        ));
    }
    let artifact_identity = candidate_artifact_identity(message.artifact_bundle.as_ref())?;
    validate_expected_artifact_identity(
        intent.expected_artifact_identity.as_deref(),
        artifact_identity.as_deref(),
    )?;
    let design = message
        .output
        .ok_or_else(|| AppError::not_found("Version source not found for runtime repair."))?;
    let version_input_identity = canonical_version_input_digest(&design, &design.initial_params)?;
    Ok(RepairCandidate {
        design,
        previous_manifest: message.model_manifest,
        version_input_identity,
        artifact_identity,
    })
}

async fn repair_with_renderer<R: VersionRuntimeRenderer>(
    intent: RepairVersionRuntimeIntent,
    state: &AppState,
    app: &dyn PathResolver,
    renderer: &R,
) -> AppResult<RepairVersionRuntimeResponse> {
    let candidate = {
        let conn = state.db.lock().await;
        load_candidate(&conn, &intent)?
    };

    let (artifact_bundle, model_manifest) = renderer
        .render(
            &candidate.design,
            candidate.previous_manifest.as_ref(),
            state,
            app,
        )
        .await?;
    validate_model_runtime_bundle(&model_manifest, &artifact_bundle)?;
    let render_snapshot = build_render_snapshot(RenderSnapshotInput {
        design: &candidate.design,
        effective_params: &candidate.design.initial_params,
        artifact_bundle: &artifact_bundle,
        model_manifest: &model_manifest,
    })?;
    let artifact_identity = artifact_bundle.content_hash.clone();

    let workspace = {
        let conn = state.db.lock().await;
        let current = load_candidate(&conn, &intent)?;
        if current.version_input_identity != candidate.version_input_identity
            || current.artifact_identity != candidate.artifact_identity
        {
            return Err(AppError::conflict(
                "Version changed while its runtime was being repaired.",
            ));
        }
        conn.execute_batch("SAVEPOINT repair_version_runtime")
            .map_err(|error| AppError::persistence(error.to_string()))?;
        let write_result = (|| {
            crate::db::update_message_artifact_bundle(&conn, &intent.message_id, &artifact_bundle)?;
            crate::db::update_message_model_manifest(&conn, &intent.message_id, &model_manifest)?;
            crate::db::update_message_structural_verification(&conn, &intent.message_id, None)?;
            Ok::<_, rusqlite::Error>(())
        })();
        if let Err(error) = write_result {
            let _ = conn.execute_batch(
                "ROLLBACK TO repair_version_runtime; RELEASE repair_version_runtime",
            );
            return Err(AppError::persistence(error.to_string()));
        }
        conn.execute_batch("RELEASE repair_version_runtime")
            .map_err(|error| AppError::persistence(error.to_string()))?;
        crate::services::history::get_workspace_projection(
            &conn,
            &intent.thread_id,
            Some(&intent.message_id),
            None,
        )?
    };

    state
        .authoring_actor_registry
        .invalidate_authoring_actors_for_thread(&intent.thread_id)
        .await;
    let last_snapshot = crate::services::session::build_saved_version_snapshot(
        Some(candidate.design),
        intent.thread_id.clone(),
        intent.message_id.clone(),
        Some(artifact_bundle),
        Some(model_manifest),
        None,
    );
    *state.last_snapshot.lock().unwrap() = Some(last_snapshot.clone());
    crate::services::session::write_last_snapshot(app, Some(&last_snapshot));
    state.emit_history_changed(
        Some(intent.thread_id),
        Some(intent.message_id),
        "versionRuntimeRepaired",
    );

    Ok(RepairVersionRuntimeResponse {
        snapshot_id: render_snapshot.snapshot_id,
        artifact_identity,
        workspace,
    })
}

pub async fn repair_version_runtime(
    intent: RepairVersionRuntimeIntent,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<RepairVersionRuntimeResponse> {
    repair_with_renderer(intent, state, app, &CanonicalVersionRuntimeRenderer).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{
        Config, DocumentMetadata, EngineKind, EnrichmentStatus, GeometryBackend, InteractionMode,
        MacroDialect, ManifestEnrichmentState, McpConfig, Message, MessageRole, MessageStatus,
        ModelSourceKind, ParamValue, SourceLanguage, UiSpec,
    };
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    struct TestResolver {
        root: PathBuf,
    }

    impl PathResolver for TestResolver {
        fn app_config_dir(&self) -> PathBuf {
            self.root.clone()
        }

        fn app_data_dir(&self) -> PathBuf {
            self.root.clone()
        }

        fn resource_path(&self, _path: &str) -> Option<PathBuf> {
            None
        }
    }

    #[derive(Clone)]
    struct FakeRenderer {
        result: AppResult<(ArtifactBundle, ModelManifest)>,
        observed: Arc<Mutex<Vec<DesignOutput>>>,
    }

    impl VersionRuntimeRenderer for FakeRenderer {
        fn render<'a>(
            &'a self,
            design: &'a DesignOutput,
            _previous_manifest: Option<&'a ModelManifest>,
            _state: &'a AppState,
            _app: &'a dyn PathResolver,
        ) -> RenderFuture<'a> {
            self.observed.lock().unwrap().push(design.clone());
            let result = self.result.clone();
            Box::pin(async move { result })
        }
    }

    fn test_config() -> Config {
        Config {
            engines: Vec::new(),
            selected_engine_id: String::new(),
            freecad_cmd: String::new(),
            cad_text_font_path: String::new(),
            freecad_library_roots: Vec::new(),
            assets: Vec::new(),
            microwave: None,
            voice: crate::contracts::VoiceConfig::default(),
            mcp: McpConfig::default(),
            fem_compute: crate::contracts::FemComputeConfig::default(),
            has_seen_onboarding: true,
            connection_type: None,
            provider_models: crate::contracts::ProviderModels::default(),
            jev_classifier: Default::default(),
            default_engine_kind: EngineKind::EckyIrV0,
            default_source_language: SourceLanguage::EckyIrV0,
            default_geometry_backend: GeometryBackend::EckyRust,
            max_generation_attempts: 3,
            max_verify_attempts: 0,
            projects_root: None,
        }
    }

    fn design() -> DesignOutput {
        DesignOutput {
            title: "Stored bracket".to_string(),
            version_name: "V1".to_string(),
            response: String::new(),
            interaction_mode: InteractionMode::Design,
            macro_code: "(model (part body (box width 10 10)))".to_string(),
            macro_dialect: MacroDialect::EckyIrV0,
            engine_kind: EngineKind::EckyIrV0,
            source_language: SourceLanguage::EckyIrV0,
            geometry_backend: GeometryBackend::EckyRust,
            ui_spec: UiSpec::default(),
            initial_params: BTreeMap::from([("width".to_string(), ParamValue::Number(27.0))]),
            post_processing: None,
        }
    }

    fn bundle(model_id: &str, root: &Path) -> ArtifactBundle {
        ArtifactBundle {
            schema_version: 1,
            model_id: model_id.to_string(),
            source_kind: ModelSourceKind::Generated,
            engine_kind: EngineKind::EckyIrV0,
            source_language: SourceLanguage::EckyIrV0,
            geometry_backend: GeometryBackend::EckyRust,
            content_hash: format!("sha256:{model_id}"),
            artifact_version: 1,
            fcstd_path: root.join(format!("{model_id}.FCStd")).display().to_string(),
            manifest_path: root.join(format!("{model_id}.json")).display().to_string(),
            macro_path: None,
            model_stl_path: root.join(format!("{model_id}.stl")).display().to_string(),
            viewer_assets: Vec::new(),
            edge_targets: Vec::new(),
            face_targets: Vec::new(),
            callout_anchors: Vec::new(),
            measurement_guides: Vec::new(),
            export_artifacts: Vec::new(),
            geometry_provenance: None,
            component_dependency_lock: None,
            component_dependency_lock_digest: None,
            component_import_origins: Vec::new(),
            component_placement_evidence: Vec::new(),
        }
    }

    fn manifest(model_id: &str) -> ModelManifest {
        ModelManifest {
            schema_version: 1,
            model_id: model_id.to_string(),
            source_kind: ModelSourceKind::Generated,
            source_digest: None,
            core_digest: None,
            ast_schema_version: None,
            engine_kind: EngineKind::EckyIrV0,
            source_language: SourceLanguage::EckyIrV0,
            geometry_backend: GeometryBackend::EckyRust,
            document: DocumentMetadata {
                document_name: "Stored bracket".to_string(),
                document_label: "Stored bracket".to_string(),
                source_path: None,
                object_count: 0,
                warnings: Vec::new(),
            },
            parts: Vec::new(),
            parameter_groups: Vec::new(),
            control_primitives: Vec::new(),
            control_relations: Vec::new(),
            control_views: Vec::new(),
            preview_views: Vec::new(),
            advisories: Vec::new(),
            selection_targets: Vec::new(),
            measurement_annotations: Vec::new(),
            tagged_anchors: BTreeMap::new(),
            feature_graph: None,
            correspondence_graph: None,
            analysis_declarations: Vec::new(),
            warnings: Vec::new(),
            enrichment_state: ManifestEnrichmentState {
                status: EnrichmentStatus::None,
                proposals: Vec::new(),
            },
            geometry_provenance: None,
            component_import_origins: Vec::new(),
            component_placement_evidence: Vec::new(),
        }
    }

    async fn fixture() -> (AppState, TestResolver, ArtifactBundle) {
        let root = std::env::temp_dir().join(format!(
            "ecky-version-runtime-repair-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let conn = crate::db::init_db(&root.join("history.sqlite")).unwrap();
        crate::db::create_or_update_thread(&conn, "thread-1", "Bracket", 1, None).unwrap();
        let old_bundle = bundle("model-missing", &root);
        crate::db::add_message(
            &conn,
            "thread-1",
            &Message {
                id: "message-1".to_string(),
                role: MessageRole::Assistant,
                content: "Version V1".to_string(),
                status: MessageStatus::Success,
                output: Some(design()),
                usage: None,
                artifact_bundle: Some(old_bundle.clone()),
                model_manifest: Some(manifest("model-missing")),
                structural_verification: None,
                agent_origin: None,
                timestamp: 1,
                image_data: None,
                visual_kind: None,
                attachment_images: Vec::new(),
            },
        )
        .unwrap();
        (
            AppState::new(test_config(), None, conn),
            TestResolver { root },
            old_bundle,
        )
    }

    fn intent(expected_artifact_identity: Option<String>) -> RepairVersionRuntimeIntent {
        RepairVersionRuntimeIntent {
            thread_id: "thread-1".to_string(),
            message_id: "message-1".to_string(),
            expected_artifact_identity,
        }
    }

    #[test]
    fn tauri_boundary_accepts_only_camel_case_repair_intent_fields() {
        let parsed: RepairVersionRuntimeIntent = serde_json::from_value(serde_json::json!({
            "threadId": "thread-1",
            "messageId": "message-1",
            "expectedArtifactIdentity": "sha256:artifact"
        }))
        .expect("camelCase intent");
        assert_eq!(parsed.thread_id, "thread-1");
        assert_eq!(
            parsed.expected_artifact_identity.as_deref(),
            Some("sha256:artifact")
        );
        assert!(
            serde_json::from_value::<RepairVersionRuntimeIntent>(serde_json::json!({
                "thread_id": "thread-1",
                "message_id": "message-1"
            }))
            .is_err()
        );
    }

    #[tokio::test]
    async fn given_missing_artifact_when_repair_intent_runs_then_exact_stored_source_and_params_are_rebuilt_and_projected(
    ) {
        let (state, resolver, old_bundle) = fixture().await;
        let observed = Arc::new(Mutex::new(Vec::new()));
        let renderer = FakeRenderer {
            result: Ok((
                bundle("model-rebuilt", &resolver.root),
                manifest("model-rebuilt"),
            )),
            observed: observed.clone(),
        };

        let response = repair_with_renderer(
            intent(Some(old_bundle.content_hash.clone())),
            &state,
            &resolver,
            &renderer,
        )
        .await
        .expect("runtime repaired");

        let seen = observed.lock().unwrap();
        assert_eq!(seen.as_slice(), &[design()]);
        assert_eq!(
            response.workspace.selected_version.as_ref().unwrap().id,
            "message-1"
        );
        assert_eq!(
            response
                .workspace
                .selected_version
                .as_ref()
                .unwrap()
                .artifact_bundle
                .as_ref()
                .unwrap()
                .model_id,
            "model-rebuilt"
        );
        assert_eq!(
            state
                .last_snapshot
                .lock()
                .unwrap()
                .as_ref()
                .and_then(|snapshot| snapshot.message_id.as_deref()),
            Some("message-1")
        );
    }

    #[tokio::test]
    async fn given_source_less_imported_legacy_version_when_repair_runs_then_raw_source_error_is_returned_without_render(
    ) {
        let (state, resolver, _) = fixture().await;
        {
            let conn = state.db.lock().await;
            conn.execute(
                "UPDATE messages SET output = NULL, version_input_digest = NULL, runtime_cache_key = NULL WHERE id = ?1",
                ["message-1"],
            )
            .unwrap();
        }
        let observed = Arc::new(Mutex::new(Vec::new()));
        let renderer = FakeRenderer {
            result: Ok((bundle("unused", &resolver.root), manifest("unused"))),
            observed: observed.clone(),
        };

        let error = repair_with_renderer(intent(None), &state, &resolver, &renderer)
            .await
            .expect_err("source-less version must fail");

        assert_eq!(
            error.message,
            "Version source not found for runtime repair."
        );
        assert!(observed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn given_stale_artifact_identity_when_repair_runs_then_conflict_prevents_render() {
        let (state, resolver, _) = fixture().await;
        let observed = Arc::new(Mutex::new(Vec::new()));
        let renderer = FakeRenderer {
            result: Ok((bundle("unused", &resolver.root), manifest("unused"))),
            observed: observed.clone(),
        };

        let error = repair_with_renderer(
            intent(Some("sha256:stale-artifact".to_string())),
            &state,
            &resolver,
            &renderer,
        )
        .await
        .expect_err("stale identity must conflict");

        assert_eq!(error.code, crate::contracts::AppErrorCode::Conflict);
        assert!(error.message.contains("expected 'sha256:stale-artifact'"));
        assert!(observed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn given_renderer_raw_failure_when_repair_runs_then_error_and_stored_runtime_remain_unchanged(
    ) {
        let (state, resolver, old_bundle) = fixture().await;
        let raw_error = AppError::with_details(
            crate::contracts::AppErrorCode::Render,
            "FreeCAD stderr: boolean cut failed",
            "PartDesign::Feature returned null shape",
        );
        let renderer = FakeRenderer {
            result: Err(raw_error.clone()),
            observed: Arc::new(Mutex::new(Vec::new())),
        };

        let error = repair_with_renderer(intent(None), &state, &resolver, &renderer)
            .await
            .expect_err("render must fail");

        assert_eq!(error, raw_error);
        let conn = state.db.lock().await;
        let stored = crate::db::get_thread_message_version(&conn, "thread-1", "message-1")
            .unwrap()
            .unwrap();
        assert_eq!(stored.artifact_bundle, Some(old_bundle));
    }
}
