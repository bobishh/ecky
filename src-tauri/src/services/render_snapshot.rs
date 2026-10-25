use crate::contracts::{
    AppError, AppResult, ArtifactBundle, ComponentDependencyLock, DesignOutput, DesignParams,
    ModelManifest, RenderSnapshot,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub struct RenderSnapshotInput<'a> {
    pub design: &'a DesignOutput,
    pub effective_params: &'a DesignParams,
    pub artifact_bundle: &'a ArtifactBundle,
    pub model_manifest: &'a ModelManifest,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotIdentity<'a> {
    source_digest: &'a str,
    parameter_digest: &'a str,
    post_processing_digest: &'a str,
    engine_kind: &'a crate::contracts::EngineKind,
    source_language: &'a crate::contracts::SourceLanguage,
    geometry_backend: &'a crate::contracts::GeometryBackend,
    artifact_content_hash: &'a str,
    artifact_digest: &'a str,
    manifest_digest: &'a str,
    component_dependency_lock_digest: Option<&'a str>,
}

fn digest_bytes(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn canonical_digest<T: Serialize>(value: &T, field: &str) -> AppResult<String> {
    serde_json::to_vec(value)
        .map(|bytes| digest_bytes(&bytes))
        .map_err(|error| AppError::validation(format!("Cannot canonicalize {field}: {error}")))
}

pub fn canonical_source_digest(source: &str) -> String {
    digest_bytes(source.as_bytes())
}

pub fn canonical_parameter_digest(params: &DesignParams) -> AppResult<String> {
    canonical_digest(params, "effectiveParams")
}

const VERSION_INPUT_DIGEST_SCHEMA: &str = "version-input-v1";
const VERSION_RUNTIME_CACHE_SCHEMA: &str = "version-runtime-v1";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VersionInputIdentity<'a> {
    schema: &'static str,
    source: &'a str,
    effective_params: &'a DesignParams,
    interaction_mode: &'a crate::contracts::InteractionMode,
    macro_dialect: &'a crate::contracts::MacroDialect,
    engine_kind: &'a crate::contracts::EngineKind,
    source_language: &'a crate::contracts::SourceLanguage,
    geometry_backend: &'a crate::contracts::GeometryBackend,
    ui_spec: &'a crate::contracts::UiSpec,
    post_processing: Option<crate::contracts::PostProcessingSpec>,
}

/// Digest of immutable version-owned render inputs. `effective_params` must be
/// the complete resolved map, never a caller patch. Result metadata, artifact
/// paths, status, labels, and timestamps intentionally do not participate.
pub fn canonical_version_input_digest(
    design: &DesignOutput,
    effective_params: &DesignParams,
) -> AppResult<String> {
    canonical_digest(
        &VersionInputIdentity {
            schema: VERSION_INPUT_DIGEST_SCHEMA,
            source: &design.macro_code,
            effective_params,
            interaction_mode: &design.interaction_mode,
            macro_dialect: &design.macro_dialect,
            engine_kind: &design.engine_kind,
            source_language: &design.source_language,
            geometry_backend: &design.geometry_backend,
            ui_spec: &design.ui_spec,
            post_processing: crate::contracts::normalize_post_processing_spec(
                design.post_processing.clone(),
            ),
        },
        "versionInput",
    )
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VersionRuntimeCacheIdentity<'a> {
    schema: &'static str,
    durable_version_id: &'a str,
    version_input_digest: &'a str,
    model_id: &'a str,
    artifact_content_hash: &'a str,
    component_dependency_lock_digest: Option<&'a str>,
}

/// Version-runtime association key. Artifact stores may deduplicate bytes, but
/// runtime ownership never aliases two durable message versions.
pub fn version_runtime_cache_key(
    durable_version_id: &str,
    version_input_digest: &str,
    artifact_bundle: &ArtifactBundle,
) -> AppResult<String> {
    if durable_version_id.trim().is_empty() {
        return Err(AppError::validation(
            "durableVersionId must not be empty for version runtime cache identity.",
        ));
    }
    canonical_digest(
        &VersionRuntimeCacheIdentity {
            schema: VERSION_RUNTIME_CACHE_SCHEMA,
            durable_version_id,
            version_input_digest,
            model_id: &artifact_bundle.model_id,
            artifact_content_hash: &artifact_bundle.content_hash,
            component_dependency_lock_digest: artifact_bundle
                .component_dependency_lock_digest
                .as_deref(),
        },
        "versionRuntimeCacheIdentity",
    )
}

pub fn artifact_bundle_digest(bundle: &ArtifactBundle) -> AppResult<String> {
    canonical_digest(bundle, "artifactBundle")
}

pub fn model_manifest_digest(manifest: &ModelManifest) -> AppResult<String> {
    canonical_digest(manifest, "modelManifest")
}

/// Canonical `sha256:<hex>` digest of a dependency lock's canonical bytes.
/// Enters `RenderSnapshot` identity and artifact cache keys so equal
/// source/params with different dependency locks cannot reuse one artifact.
pub fn component_dependency_lock_digest(lock: &ComponentDependencyLock) -> AppResult<String> {
    canonical_digest(&lock.clone().canonical(), "componentDependencyLock")
}

pub fn build_render_snapshot(input: RenderSnapshotInput<'_>) -> AppResult<RenderSnapshot> {
    validate_render_compatibility(input.design, input.artifact_bundle, input.model_manifest)?;

    let source_digest = canonical_source_digest(&input.design.macro_code);
    if let Some(manifest_source_digest) = input.model_manifest.source_digest.as_deref() {
        if manifest_source_digest != source_digest {
            return Err(AppError::validation(format!(
                "sourceDigest mismatch: design source digest '{}' conflicts with modelManifest.sourceDigest '{}'.",
                source_digest, manifest_source_digest
            )));
        }
    }
    let parameter_digest = canonical_parameter_digest(input.effective_params)?;
    let post_processing_digest = canonical_digest(&input.design.post_processing, "postProcessing")?;
    let artifact_digest = artifact_bundle_digest(input.artifact_bundle)?;
    let manifest_digest = model_manifest_digest(input.model_manifest)?;
    let identity = SnapshotIdentity {
        source_digest: &source_digest,
        parameter_digest: &parameter_digest,
        post_processing_digest: &post_processing_digest,
        engine_kind: &input.design.engine_kind,
        source_language: &input.design.source_language,
        geometry_backend: &input.design.geometry_backend,
        artifact_content_hash: &input.artifact_bundle.content_hash,
        artifact_digest: &artifact_digest,
        manifest_digest: &manifest_digest,
        component_dependency_lock_digest: input
            .artifact_bundle
            .component_dependency_lock_digest
            .as_deref(),
    };
    let snapshot_id = canonical_digest(&identity, "snapshot identity")?;

    Ok(RenderSnapshot {
        snapshot_id,
        model_id: input.artifact_bundle.model_id.clone(),
        source: input.design.macro_code.clone(),
        source_digest,
        effective_params: input.effective_params.clone(),
        parameter_digest,
        post_processing: input.design.post_processing.clone(),
        post_processing_digest,
        engine_kind: input.design.engine_kind,
        source_language: input.design.source_language,
        geometry_backend: input.design.geometry_backend,
        artifact_bundle: input.artifact_bundle.clone(),
        artifact_digest,
        model_manifest: input.model_manifest.clone(),
        manifest_digest,
        component_dependency_lock_digest: input
            .artifact_bundle
            .component_dependency_lock_digest
            .clone(),
    })
}

pub fn validate_render_snapshot(snapshot: &RenderSnapshot) -> AppResult<()> {
    let design = DesignOutput {
        title: String::new(),
        version_name: String::new(),
        response: String::new(),
        interaction_mode: crate::contracts::InteractionMode::Design,
        macro_code: snapshot.source.clone(),
        macro_dialect: crate::contracts::MacroDialect::Legacy,
        engine_kind: snapshot.engine_kind,
        source_language: snapshot.source_language,
        geometry_backend: snapshot.geometry_backend,
        ui_spec: crate::contracts::UiSpec::default(),
        initial_params: snapshot.effective_params.clone(),
        post_processing: snapshot.post_processing.clone(),
    };
    let rebuilt = build_render_snapshot(RenderSnapshotInput {
        design: &design,
        effective_params: &snapshot.effective_params,
        artifact_bundle: &snapshot.artifact_bundle,
        model_manifest: &snapshot.model_manifest,
    })?;
    if rebuilt.model_id != snapshot.model_id
        || rebuilt.snapshot_id != snapshot.snapshot_id
        || rebuilt.source_digest != snapshot.source_digest
        || rebuilt.parameter_digest != snapshot.parameter_digest
        || rebuilt.post_processing_digest != snapshot.post_processing_digest
        || rebuilt.artifact_digest != snapshot.artifact_digest
        || rebuilt.manifest_digest != snapshot.manifest_digest
    {
        return Err(AppError::validation(format!(
            "Render snapshot '{}' digest mismatch; payload no longer matches its canonical render identity.",
            snapshot.snapshot_id
        )));
    }
    Ok(())
}

pub(crate) fn validate_render_compatibility(
    design: &DesignOutput,
    bundle: &ArtifactBundle,
    manifest: &ModelManifest,
) -> AppResult<()> {
    crate::contracts::validate_component_import_evidence(
        bundle.component_dependency_lock.as_ref(),
        bundle.component_dependency_lock_digest.as_deref(),
        &bundle.component_import_origins,
        &manifest.component_import_origins,
    )?;
    if bundle.model_id != manifest.model_id {
        return Err(AppError::validation(format!(
            "modelId mismatch: artifactBundle.modelId '{}' conflicts with modelManifest.modelId '{}'.",
            bundle.model_id, manifest.model_id
        )));
    }
    if design.engine_kind != bundle.engine_kind || design.engine_kind != manifest.engine_kind {
        return Err(AppError::validation(
            "engineKind mismatch across design, artifactBundle, and modelManifest.",
        ));
    }
    if design.source_language != bundle.source_language
        || design.source_language != manifest.source_language
    {
        return Err(AppError::validation(
            "sourceLanguage mismatch across design, artifactBundle, and modelManifest.",
        ));
    }
    if design.geometry_backend != bundle.geometry_backend
        || design.geometry_backend != manifest.geometry_backend
    {
        return Err(AppError::validation(
            "geometryBackend mismatch across design, artifactBundle, and modelManifest.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{
        DocumentMetadata, EngineKind, EnrichmentStatus, GeometryBackend, InteractionMode,
        MacroDialect, ManifestEnrichmentState, ModelSourceKind, ParamValue, SourceLanguage, UiSpec,
        VerificationRecord, VerifierStatus,
    };
    use std::collections::BTreeMap;

    fn design() -> DesignOutput {
        DesignOutput {
            title: "Snapshot".to_string(),
            version_name: "V1".to_string(),
            response: String::new(),
            interaction_mode: InteractionMode::Design,
            macro_code: "(model (part body (box width 10 10)))".to_string(),
            macro_dialect: MacroDialect::EckyIrV0,
            engine_kind: EngineKind::EckyIrV0,
            source_language: SourceLanguage::EckyIrV0,
            geometry_backend: GeometryBackend::EckyRust,
            ui_spec: UiSpec::default(),
            initial_params: BTreeMap::new(),
            post_processing: None,
        }
    }

    fn bundle(model_id: &str) -> ArtifactBundle {
        ArtifactBundle {
            schema_version: 1,
            model_id: model_id.to_string(),
            source_kind: ModelSourceKind::Generated,
            engine_kind: EngineKind::EckyIrV0,
            source_language: SourceLanguage::EckyIrV0,
            geometry_backend: GeometryBackend::EckyRust,
            content_hash: "sha256:artifact".to_string(),
            artifact_version: 1,
            fcstd_path: String::new(),
            manifest_path: String::new(),
            macro_path: None,
            model_stl_path: String::new(),
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
                document_name: "Snapshot".to_string(),
                document_label: "Snapshot".to_string(),
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

    #[test]
    fn canonical_parameter_digest_is_stable_across_insertion_order() {
        let first = BTreeMap::from([
            ("width".to_string(), ParamValue::Number(12.0)),
            ("enabled".to_string(), ParamValue::Boolean(true)),
        ]);
        let second = BTreeMap::from([
            ("enabled".to_string(), ParamValue::Boolean(true)),
            ("width".to_string(), ParamValue::Number(12.0)),
        ]);

        assert_eq!(
            canonical_parameter_digest(&first).expect("first digest"),
            canonical_parameter_digest(&second).expect("second digest")
        );
    }

    #[test]
    fn version_input_digest_covers_full_effective_parameter_map_canonically() {
        let design = design();
        let first = BTreeMap::from([
            ("width".to_string(), ParamValue::Number(12.0)),
            ("enabled".to_string(), ParamValue::Boolean(true)),
        ]);
        let reordered = BTreeMap::from([
            ("enabled".to_string(), ParamValue::Boolean(true)),
            ("width".to_string(), ParamValue::Number(12.0)),
        ]);
        let changed = BTreeMap::from([
            ("width".to_string(), ParamValue::Number(13.0)),
            ("enabled".to_string(), ParamValue::Boolean(true)),
        ]);

        let first_digest = canonical_version_input_digest(&design, &first).expect("first");
        let reordered_digest =
            canonical_version_input_digest(&design, &reordered).expect("reordered");
        let changed_digest = canonical_version_input_digest(&design, &changed).expect("changed");

        assert_eq!(first_digest, reordered_digest);
        assert_ne!(first_digest, changed_digest);
    }

    #[test]
    fn version_runtime_cache_key_is_scoped_to_durable_version() {
        let input_digest = canonical_version_input_digest(&design(), &BTreeMap::new())
            .expect("version input digest");

        let artifact = bundle("model-a");
        let first = version_runtime_cache_key("message-a", &input_digest, &artifact)
            .expect("first cache key");
        let second = version_runtime_cache_key("message-b", &input_digest, &artifact)
            .expect("second cache key");

        assert_ne!(first, second);
    }

    #[test]
    fn version_runtime_cache_key_rejects_changed_artifact_content() {
        let input_digest = canonical_version_input_digest(&design(), &BTreeMap::new())
            .expect("version input digest");
        let first_artifact = bundle("model-a");
        let mut changed_artifact = first_artifact.clone();
        changed_artifact.content_hash = "sha256:changed-artifact".to_string();

        let first = version_runtime_cache_key("message-a", &input_digest, &first_artifact)
            .expect("first cache key");
        let changed = version_runtime_cache_key("message-a", &input_digest, &changed_artifact)
            .expect("changed cache key");

        assert_ne!(first, changed);
    }

    #[test]
    fn snapshot_rejects_model_backend_and_source_mismatch() {
        let design = design();
        let params = BTreeMap::new();
        let err = build_render_snapshot(RenderSnapshotInput {
            design: &design,
            effective_params: &params,
            artifact_bundle: &bundle("model-a"),
            model_manifest: &manifest("model-b"),
        })
        .expect_err("model mismatch");
        assert!(err.message.contains("modelId mismatch"));

        let mut backend_bundle = bundle("model-a");
        backend_bundle.geometry_backend = GeometryBackend::Build123d;
        let err = build_render_snapshot(RenderSnapshotInput {
            design: &design,
            effective_params: &params,
            artifact_bundle: &backend_bundle,
            model_manifest: &manifest("model-a"),
        })
        .expect_err("backend mismatch");
        assert!(err.message.contains("geometryBackend mismatch"));

        let mut source_manifest = manifest("model-a");
        source_manifest.source_language = SourceLanguage::Build123d;
        let err = build_render_snapshot(RenderSnapshotInput {
            design: &design,
            effective_params: &params,
            artifact_bundle: &bundle("model-a"),
            model_manifest: &source_manifest,
        })
        .expect_err("source mismatch");
        assert!(err.message.contains("sourceLanguage mismatch"));
    }

    #[test]
    fn snapshot_and_verification_record_use_camel_case_contract_fields() {
        let design = design();
        let params = BTreeMap::from([("width".to_string(), ParamValue::Number(12.0))]);
        let snapshot = build_render_snapshot(RenderSnapshotInput {
            design: &design,
            effective_params: &params,
            artifact_bundle: &bundle("model-a"),
            model_manifest: &manifest("model-a"),
        })
        .expect("valid snapshot");
        validate_render_snapshot(&snapshot).expect("validates canonical payload");

        let snapshot_json = serde_json::to_value(&snapshot).expect("serialize snapshot");
        assert!(snapshot_json.get("snapshotId").is_some());
        assert!(snapshot_json.get("effectiveParams").is_some());
        assert!(snapshot_json.get("snapshot_id").is_none());

        let record = VerificationRecord {
            verification_id: "verify-1".to_string(),
            snapshot_id: snapshot.snapshot_id,
            artifact_digest: snapshot.artifact_digest,
            passed: true,
            verifier_status: VerifierStatus::Ok,
            verifier_source: None,
        };
        let record_json = serde_json::to_value(record).expect("serialize verification");
        assert!(record_json.get("verificationId").is_some());
        assert!(record_json.get("artifactDigest").is_some());
        assert!(record_json.get("verification_id").is_none());
    }

    #[test]
    fn component_dependency_lock_digest_is_independent_of_input_order() {
        use crate::contracts::{
            ComponentDependencyLock, ComponentDependencyLockComponent,
            ComponentDependencyLockEntry, COMPONENT_DEPENDENCY_LOCK_SCHEMA_VERSION,
        };

        fn lock(payload: &str) -> ComponentDependencyLock {
            ComponentDependencyLock {
                cairn: None,
                schema_version: COMPONENT_DEPENDENCY_LOCK_SCHEMA_VERSION,
                dependencies: vec![ComponentDependencyLockEntry {
                    package_id: "bike.kit".to_string(),
                    version: "1.2.0".to_string(),
                    package_digest: payload.to_string(),
                    components: vec![ComponentDependencyLockComponent {
                        component_id: "cage".to_string(),
                        entry_symbol: None,
                        payload_digest: payload.to_string(),
                        payload_kind: None,
                        geometry_representation: None,
                    }],
                }],
            }
        }

        let a = component_dependency_lock_digest(&lock("sha256:aaa")).expect("digest a");
        let b = component_dependency_lock_digest(&lock("sha256:aaa")).expect("digest b");
        let c = component_dependency_lock_digest(&lock("sha256:bbb")).expect("digest c");

        assert_eq!(a, b, "identical lock content must hash identically");
        assert_ne!(
            a, c,
            "different payload digests must change the lock digest"
        );
    }

    #[test]
    fn snapshot_identity_separates_equal_source_and_params_by_dependency_lock() {
        use crate::contracts::{
            ComponentDependencyLockComponent, ComponentDependencyLockEntry, ComponentPayloadKind,
            COMPONENT_DEPENDENCY_LOCK_SCHEMA_VERSION,
        };

        fn locked_bundle(model_id: &str, payload: &str) -> ArtifactBundle {
            let mut bundle = bundle(model_id);
            let lock = ComponentDependencyLock {
                cairn: None,
                schema_version: COMPONENT_DEPENDENCY_LOCK_SCHEMA_VERSION,
                dependencies: vec![ComponentDependencyLockEntry {
                    package_id: "fixture.live".to_string(),
                    version: "1.0.0".to_string(),
                    package_digest: payload.to_string(),
                    components: vec![ComponentDependencyLockComponent {
                        component_id: "cage".to_string(),
                        entry_symbol: Some("cage".to_string()),
                        payload_digest: payload.to_string(),
                        payload_kind: Some(ComponentPayloadKind::Source),
                        geometry_representation: None,
                    }],
                }],
            };
            bundle.component_dependency_lock_digest =
                Some(component_dependency_lock_digest(&lock).expect("lock digest"));
            bundle.component_dependency_lock = Some(lock);
            bundle
        }

        let design = design();
        let params = BTreeMap::new();
        let first = locked_bundle("model-a", &format!("sha256:{}", "a".repeat(64)));
        let second = locked_bundle("model-a", &format!("sha256:{}", "b".repeat(64)));
        let first_snapshot = build_render_snapshot(RenderSnapshotInput {
            design: &design,
            effective_params: &params,
            artifact_bundle: &first,
            model_manifest: &manifest("model-a"),
        })
        .expect("first snapshot");
        let second_snapshot = build_render_snapshot(RenderSnapshotInput {
            design: &design,
            effective_params: &params,
            artifact_bundle: &second,
            model_manifest: &manifest("model-a"),
        })
        .expect("second snapshot");

        assert_ne!(
            first_snapshot.component_dependency_lock_digest,
            second_snapshot.component_dependency_lock_digest
        );
        assert_ne!(first_snapshot.snapshot_id, second_snapshot.snapshot_id);
    }
}
