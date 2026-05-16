use crate::contracts::infer_macro_dialect_from_code;
use crate::freecad;
use crate::models::{
    AppError, AppResult, AppState, ArtifactBundle, DesignParams, GeometryBackend, MacroDialect,
    ModelManifest, PathResolver,
};
use std::fs;
use std::path::Path;

fn load_manifest_for_bundle(bundle: &ArtifactBundle) -> AppResult<Option<ModelManifest>> {
    let path = bundle.manifest_path.trim();
    if path.is_empty() {
        return Ok(None);
    }
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(AppError::internal(format!(
                "Failed to read model manifest '{}': {}",
                path, err
            )))
        }
    };
    let parsed: ModelManifest = serde_json::from_str(&raw).map_err(|e| {
        AppError::internal(format!("Failed to parse model manifest '{}': {}", path, e))
    })?;
    Ok(Some(parsed))
}

fn update_content_hash_and_exports(
    preview_stl_path: &str,
    bundle: &mut ArtifactBundle,
) -> AppResult<()> {
    let stl_path = Path::new(preview_stl_path);
    if let Ok(bytes) = std::fs::read(stl_path) {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        bundle.content_hash = format!("{:x}", hasher.finalize());
    }
    Ok(())
}

fn apply_requested_post_processing(
    bundle: &mut ArtifactBundle,
    parameters: &DesignParams,
    post_processing: Option<&crate::contracts::PostProcessingSpec>,
) -> AppResult<()> {
    let Some(post_proc) =
        crate::contracts::normalize_post_processing_spec(post_processing.cloned())
    else {
        return Ok(());
    };
    let has_explicit_attachment_path = post_processing
        .map(|post| !post.lithophane_attachments.is_empty())
        .unwrap_or(false);

    let stl_path = Path::new(&bundle.preview_stl_path);

    if has_explicit_attachment_path && !post_proc.lithophane_attachments.is_empty() {
        let resolved_attachments =
            resolve_lithophane_attachments(bundle, parameters, &post_proc.lithophane_attachments)?;

        if !resolved_attachments.is_empty() {
            let export_dir = crate::lithophane::export_dir_for_preview(stl_path);
            bundle.export_artifacts.clear();
            bundle.export_artifacts = crate::lithophane::apply_lithophane_attachments(
                stl_path,
                &resolved_attachments,
                stl_path,
                &export_dir,
            )?;
            let preview_path = bundle.preview_stl_path.clone();
            update_content_hash_and_exports(&preview_path, bundle)?;
            return Ok(());
        }
    }

    if let Some(disp) = &post_proc.displacement {
        let Some(crate::models::ParamValue::String(image_path)) = parameters.get(&disp.image_param)
        else {
            return Ok(());
        };
        if image_path.trim().is_empty() {
            return Ok(());
        }
        crate::displacement::apply(stl_path, image_path, disp, stl_path)?;
        bundle.export_artifacts.clear();
        let preview_path = bundle.preview_stl_path.clone();
        update_content_hash_and_exports(&preview_path, bundle)?;
    }

    Ok(())
}

fn resolve_lithophane_attachments(
    bundle: &ArtifactBundle,
    parameters: &DesignParams,
    attachments: &[crate::contracts::LithophaneAttachment],
) -> AppResult<Vec<crate::lithophane::ResolvedLithophaneAttachment>> {
    let manifest = load_manifest_for_bundle(bundle)?;
    let mut resolved = Vec::new();

    for attachment in attachments.iter().filter(|attachment| attachment.enabled) {
        let Some(image_path) = crate::lithophane::resolve_image_path(attachment, parameters) else {
            continue;
        };

        let target_part_id = attachment.target_part_id.trim();
        let target_bounds = if target_part_id.is_empty() {
            None
        } else {
            let loaded_manifest = manifest.as_ref().ok_or_else(|| {
                AppError::validation(format!(
                    "Lithophane attachment '{}' references targetPartId '{}' but the model manifest is missing.",
                    attachment.id, target_part_id
                ))
            })?;
            let target_part = loaded_manifest
                .parts
                .iter()
                .find(|part| part.part_id == target_part_id)
                .ok_or_else(|| {
                    AppError::validation(format!(
                        "Lithophane attachment '{}' references missing targetPartId '{}'.",
                        attachment.id, target_part_id
                    ))
                })?;
            Some(target_part.bounds.clone().ok_or_else(|| {
                AppError::validation(format!(
                    "Lithophane attachment '{}' targetPartId '{}' has no bounds in the model manifest.",
                    attachment.id, target_part_id
                ))
            })?)
        };

        resolved.push(crate::lithophane::ResolvedLithophaneAttachment {
            id: attachment.id.clone(),
            image_path,
            target_bounds,
            placement: attachment.placement.clone(),
            relief: attachment.relief.clone(),
            color_mode: attachment.color.mode,
            channel_thickness_mm: attachment.color.channel_thickness_mm,
        });
    }

    Ok(resolved)
}

pub fn configured_freecad_cmd(state: &AppState) -> Option<String> {
    let config = state.config.lock().unwrap();
    let cmd = config.freecad_cmd.trim();
    if cmd.is_empty() {
        None
    } else {
        Some(cmd.to_string())
    }
}

pub fn is_freecad_available(state: &AppState) -> bool {
    freecad::resolve_freecad_path(configured_freecad_cmd(state).as_deref()).is_ok()
}

fn finalize_render_bundle(
    mut bundle: ArtifactBundle,
    parameters: &DesignParams,
    post_processing: Option<&crate::contracts::PostProcessingSpec>,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    apply_requested_post_processing(&mut bundle, parameters, post_processing)?;
    let runtime_cache_dir = freecad::runtime_cache_dir(app)?;
    freecad::evict_cache_if_needed(&runtime_cache_dir);
    Ok(bundle)
}

fn resolve_geometry_backend(
    effective_dialect: &MacroDialect,
    requested_backend: Option<GeometryBackend>,
    config_default_backend: GeometryBackend,
) -> GeometryBackend {
    requested_backend.unwrap_or(match effective_dialect {
        MacroDialect::EckyIrV0 => config_default_backend,
        MacroDialect::Build123d => GeometryBackend::Build123d,
        MacroDialect::CadFrameworkV1 => GeometryBackend::Freecad,
        MacroDialect::Legacy => GeometryBackend::Freecad,
    })
}

fn resolve_dispatch_backend(
    macro_code: &str,
    effective_dialect: &MacroDialect,
    requested_backend: GeometryBackend,
) -> AppResult<GeometryBackend> {
    if *effective_dialect != MacroDialect::EckyIrV0 {
        return Ok(requested_backend);
    }

    let uses_mesh_only = crate::ecky_ir::source_uses_ecky_rust_only_cad_ops(macro_code);
    let uses_exact_only = crate::ecky_ir::source_uses_exact_backend_only_cad_ops(macro_code);

    if uses_mesh_only && uses_exact_only {
        return Err(AppError::validation(
            "Mesh-only ops like `wall-pattern` cannot mix with exact-only ops like `sampled-radial-loft` in one `.ecky` model.",
        ));
    }

    if matches!(
        requested_backend,
        GeometryBackend::Build123d | GeometryBackend::Freecad
    ) && uses_mesh_only
    {
        return Ok(GeometryBackend::EckyRust);
    }

    Ok(requested_backend)
}

fn try_render_direct_occt_ecky_ir(
    macro_code: &str,
    parameters: &DesignParams,
    effective_dialect: &MacroDialect,
    app: &dyn PathResolver,
) -> Option<ArtifactBundle> {
    if *effective_dialect != MacroDialect::EckyIrV0 {
        return None;
    }
    let program = crate::ecky_scheme::compile_to_core_program(macro_code).ok()?;
    let runtime_root = crate::runtime_capabilities::resolve_direct_occt_runtime_root(app).ok()?;
    let layout =
        crate::ecky_cad_host::direct_occt_sdk::inspect_build123d_ocp_runtime(&runtime_root);
    if !layout.can_compile_native_shim() {
        return None;
    }
    crate::ecky_cad_host::direct_occt_runtime::render_core_program_runtime_bundle(
        &program, macro_code, parameters, &layout, app,
    )
    .ok()
    .map(|(bundle, _manifest)| bundle)
}

pub async fn render_stl(
    macro_code: &str,
    parameters: &DesignParams,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<String> {
    let _guard = state.render_lock.lock().await;
    let result = freecad::render(
        macro_code,
        parameters,
        configured_freecad_cmd(state).as_deref(),
        app,
    );
    if result.is_ok() {
        let runtime_cache_dir = freecad::runtime_cache_dir(app)?;
        freecad::evict_cache_if_needed(&runtime_cache_dir);
    }
    result
}

pub async fn render_model(
    macro_code: &str,
    parameters: &DesignParams,
    macro_dialect: Option<MacroDialect>,
    geometry_backend: Option<GeometryBackend>,
    post_processing: Option<&crate::contracts::PostProcessingSpec>,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    let _guard = state.render_lock.lock().await;
    render_model_unlocked(
        macro_code,
        parameters,
        macro_dialect,
        geometry_backend,
        post_processing,
        state,
        app,
    )
}

fn render_model_unlocked(
    macro_code: &str,
    parameters: &DesignParams,
    macro_dialect: Option<MacroDialect>,
    geometry_backend: Option<GeometryBackend>,
    post_processing: Option<&crate::contracts::PostProcessingSpec>,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    let effective_dialect =
        macro_dialect.unwrap_or_else(|| infer_macro_dialect_from_code(macro_code));
    let config_default_backend = state.config.lock().unwrap().default_geometry_backend;
    let resolved_backend =
        resolve_geometry_backend(&effective_dialect, geometry_backend, config_default_backend);
    let dispatch_backend =
        resolve_dispatch_backend(macro_code, &effective_dialect, resolved_backend)?;
    crate::runtime_capabilities::ensure_backend_available(
        dispatch_backend,
        configured_freecad_cmd(state).as_deref(),
        app,
    )?;
    // Lower Ecky IR to the target backend before dispatch.
    // Legacy Python and Build123d sources stay as-is.
    let lowered = match (dispatch_backend, effective_dialect.clone()) {
        (GeometryBackend::Build123d, MacroDialect::EckyIrV0) => {
            Some(crate::ecky_ir::lower_to_build123d(macro_code)?)
        }
        (GeometryBackend::Freecad, MacroDialect::EckyIrV0) => {
            Some(crate::ecky_ir::lower_to_freecad(macro_code)?)
        }
        _ => None,
    };
    let dispatch_source = lowered.as_deref().unwrap_or(macro_code);
    let result = match dispatch_backend {
        GeometryBackend::EckyRust => {
            try_render_direct_occt_ecky_ir(macro_code, parameters, &effective_dialect, app)
                .map(Ok)
                .unwrap_or_else(|| {
                    if effective_dialect == MacroDialect::EckyIrV0
                        && crate::ecky_ir::source_uses_exact_backend_only_cad_ops(macro_code)
                    {
                        let lowered = crate::ecky_ir::lower_to_build123d(macro_code)?;
                        crate::build123d::render_model_with_sources(
                            &lowered,
                            Some(macro_code),
                            parameters,
                            app,
                            crate::models::SourceLanguage::EckyIrV0,
                        )
                    } else {
                        crate::ecky_ir::render_model(macro_code, parameters, app)
                    }
                })
        }
        GeometryBackend::Build123d => {
            let source_language = if effective_dialect == MacroDialect::EckyIrV0 {
                crate::models::SourceLanguage::EckyIrV0
            } else {
                crate::models::SourceLanguage::Build123d
            };
            crate::build123d::render_model_with_sources(
                dispatch_source,
                if effective_dialect == MacroDialect::EckyIrV0 {
                    Some(macro_code)
                } else {
                    None
                },
                parameters,
                app,
                source_language,
            )
        }
        GeometryBackend::Freecad => {
            let source_language = if effective_dialect == MacroDialect::EckyIrV0 {
                crate::models::SourceLanguage::EckyIrV0
            } else {
                crate::models::SourceLanguage::LegacyPython
            };
            freecad::render_model_with_sources(
                dispatch_source,
                if effective_dialect == MacroDialect::EckyIrV0 {
                    Some(macro_code)
                } else {
                    None
                },
                parameters,
                configured_freecad_cmd(state).as_deref(),
                app,
                source_language,
            )
        }
    };
    result.and_then(|bundle| finalize_render_bundle(bundle, parameters, post_processing, app))
}

pub async fn render_model_source(
    source_path: &Path,
    source_language: Option<crate::models::SourceLanguage>,
    macro_dialect: Option<MacroDialect>,
    geometry_backend: Option<GeometryBackend>,
    parameters: &DesignParams,
    post_processing: Option<&crate::contracts::PostProcessingSpec>,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    let _guard = state.render_lock.lock().await;
    render_model_source_unlocked(
        source_path,
        source_language,
        macro_dialect,
        geometry_backend,
        parameters,
        post_processing,
        state,
        app,
    )
}

fn render_model_source_unlocked(
    source_path: &Path,
    source_language: Option<crate::models::SourceLanguage>,
    macro_dialect: Option<MacroDialect>,
    geometry_backend: Option<GeometryBackend>,
    parameters: &DesignParams,
    post_processing: Option<&crate::contracts::PostProcessingSpec>,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    let extension = source_path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());
    let source_path_text = source_path
        .to_str()
        .ok_or_else(|| AppError::internal("Invalid component source path."))?;

    let bundle = match extension.as_deref() {
        Some("fcstd") => freecad::import_fcstd(
            source_path_text,
            configured_freecad_cmd(state).as_deref(),
            app,
        )?,
        Some("step") | Some("stp") => freecad::import_step(
            source_path_text,
            configured_freecad_cmd(state).as_deref(),
            app,
        )?,
        Some("ecky") | Some("py") | Some("fcmacro") | None => {
            let macro_code = fs::read_to_string(source_path).map_err(|err| {
                AppError::persistence(format!(
                    "Failed to read component source '{}': {}",
                    source_path.display(),
                    err
                ))
            })?;
            let resolved_dialect = resolve_source_macro_dialect(
                source_path,
                source_language,
                macro_dialect,
                &macro_code,
            );
            return render_model_unlocked(
                &macro_code,
                parameters,
                Some(resolved_dialect),
                geometry_backend,
                post_processing,
                state,
                app,
            );
        }
        Some(other) => {
            return Err(AppError::validation(format!(
                "Unsupported component source '{}' with extension '.{}'. Expected .ecky, .py, .FCMacro, .FCStd, or .step.",
                source_path.display(),
                other
            )));
        }
    };

    finalize_render_bundle(bundle, parameters, post_processing, app)
}

fn resolve_source_macro_dialect(
    source_path: &Path,
    source_language: Option<crate::models::SourceLanguage>,
    macro_dialect: Option<MacroDialect>,
    macro_code: &str,
) -> MacroDialect {
    if let Some(explicit) = macro_dialect {
        return explicit;
    }
    if let Some(language) = source_language {
        return match language {
            crate::models::SourceLanguage::LegacyPython => {
                infer_macro_dialect_from_code(macro_code)
            }
            crate::models::SourceLanguage::EckyIrV0 => MacroDialect::EckyIrV0,
            crate::models::SourceLanguage::Build123d => MacroDialect::Build123d,
        };
    }
    match source_path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("ecky") => MacroDialect::EckyIrV0,
        _ => infer_macro_dialect_from_code(macro_code),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_requested_post_processing, load_manifest_for_bundle, render_model,
        resolve_dispatch_backend, resolve_geometry_backend,
    };
    use crate::contracts::{
        Config, DisplacementSpec, LithophaneAttachment, LithophaneAttachmentSource,
        LithophaneColor, LithophaneColorMode, LithophanePlacement, LithophanePlacementMode,
        LithophaneRelief, LithophaneSide, MacroDialect, McpConfig, OverflowMode,
        PostProcessingSpec, ProjectionType,
    };
    use crate::models::{AppState, DesignParams, GeometryBackend, ParamValue, PathResolver};
    use std::path::PathBuf;

    #[derive(Clone)]
    struct TestResolver {
        root: PathBuf,
    }

    impl PathResolver for TestResolver {
        fn app_config_dir(&self) -> PathBuf {
            self.root.join("config")
        }

        fn app_data_dir(&self) -> PathBuf {
            self.root.join("data")
        }

        fn resource_path(&self, _path: &str) -> Option<PathBuf> {
            None
        }
    }

    fn temp_root(label: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("ecky-render-{label}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("temp root");
        root
    }

    fn test_config() -> Config {
        Config {
            engines: Vec::new(),
            selected_engine_id: String::new(),
            freecad_cmd: String::new(),
            assets: Vec::new(),
            microwave: None,
            voice: crate::models::VoiceConfig::default(),
            mcp: McpConfig::default(),
            has_seen_onboarding: true,
            connection_type: None,
            default_engine_kind: crate::models::EngineKind::Freecad,
            default_source_language: crate::models::SourceLanguage::LegacyPython,
            default_geometry_backend: GeometryBackend::Freecad,
            max_generation_attempts: 3,
            max_verify_attempts: 0,
        }
    }

    fn test_state(root: &std::path::Path) -> AppState {
        let conn = crate::db::init_db(&root.join("test.db")).expect("test db");
        AppState::new(test_config(), None, conn)
    }

    #[test]
    fn apply_requested_displacement_surfaces_raw_displacement_errors() {
        let params = DesignParams::from([(
            "image".to_string(),
            crate::models::ParamValue::String("/definitely/missing/lithophane.png".to_string()),
        )]);
        let mut bundle = crate::models::ArtifactBundle {
            schema_version: 1,
            model_id: "model".to_string(),
            source_kind: crate::models::ModelSourceKind::Generated,
            engine_kind: crate::models::EngineKind::Freecad,
            source_language: crate::models::SourceLanguage::LegacyPython,
            geometry_backend: crate::models::GeometryBackend::Freecad,
            content_hash: "unchanged".to_string(),
            artifact_version: 1,
            fcstd_path: "/tmp/model.FCStd".to_string(),
            manifest_path: "/tmp/missing-manifest.json".to_string(),
            macro_path: None,
            preview_stl_path: "/tmp/nonexistent-preview.stl".to_string(),
            viewer_assets: vec![],
            edge_targets: vec![],
            face_targets: vec![],
            callout_anchors: vec![],
            measurement_guides: vec![],
            export_artifacts: vec![],
        };

        let error = apply_requested_post_processing(
            &mut bundle,
            &params,
            Some(&PostProcessingSpec {
                displacement: Some(DisplacementSpec {
                    image_param: "image".to_string(),
                    projection: ProjectionType::Planar,
                    depth_mm: 1.0,
                    invert: false,
                }),
                lithophane_attachments: vec![],
            }),
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("Failed to open image for displacement"),
            "unexpected error: {}",
            error
        );
        assert_eq!(bundle.content_hash, "unchanged");
    }

    #[test]
    fn post_processing_noop_preserves_existing_step_export_artifacts() {
        let params = DesignParams::new();
        let mut bundle = crate::models::ArtifactBundle {
            schema_version: 1,
            model_id: "model".to_string(),
            source_kind: crate::models::ModelSourceKind::Generated,
            engine_kind: crate::models::EngineKind::EckyIrV0,
            source_language: crate::models::SourceLanguage::EckyIrV0,
            geometry_backend: crate::models::GeometryBackend::EckyRust,
            content_hash: "unchanged".to_string(),
            artifact_version: 1,
            fcstd_path: String::new(),
            manifest_path: "/tmp/missing-manifest.json".to_string(),
            macro_path: None,
            preview_stl_path: "/tmp/nonexistent-preview.stl".to_string(),
            viewer_assets: vec![],
            edge_targets: vec![],
            face_targets: vec![],
            callout_anchors: vec![],
            measurement_guides: vec![],
            export_artifacts: vec![crate::models::ExportArtifact {
                label: "STEP".to_string(),
                format: "step".to_string(),
                path: "/tmp/model.step".to_string(),
                role: "primary".to_string(),
            }],
        };

        apply_requested_post_processing(
            &mut bundle,
            &params,
            Some(&PostProcessingSpec {
                displacement: Some(DisplacementSpec {
                    image_param: "missing_image".to_string(),
                    projection: ProjectionType::Planar,
                    depth_mm: 1.0,
                    invert: false,
                }),
                lithophane_attachments: vec![],
            }),
        )
        .expect("post-processing no-op");

        assert_eq!(bundle.export_artifacts.len(), 1);
        assert_eq!(bundle.export_artifacts[0].format, "step");
        assert_eq!(bundle.export_artifacts[0].path, "/tmp/model.step");
    }

    #[test]
    fn planar_cmyk_requires_attachment_render_path_not_legacy_displacement() {
        let root = std::env::temp_dir().join(format!("ecky-litho-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let preview_stl_path = root.join("preview.stl");
        std::fs::write(
            &preview_stl_path,
            [&[0u8; 80][..], &0u32.to_le_bytes()[..]].concat(),
        )
        .unwrap();

        let params = DesignParams::new();
        let mut bundle = crate::models::ArtifactBundle {
            schema_version: 1,
            model_id: "model".to_string(),
            source_kind: crate::models::ModelSourceKind::Generated,
            engine_kind: crate::models::EngineKind::Freecad,
            source_language: crate::models::SourceLanguage::LegacyPython,
            geometry_backend: crate::models::GeometryBackend::Freecad,
            content_hash: "unchanged".to_string(),
            artifact_version: 1,
            fcstd_path: "/tmp/model.FCStd".to_string(),
            manifest_path: "/tmp/missing-manifest.json".to_string(),
            macro_path: None,
            preview_stl_path: preview_stl_path.to_string_lossy().to_string(),
            viewer_assets: vec![],
            edge_targets: vec![],
            face_targets: vec![],
            callout_anchors: vec![],
            measurement_guides: vec![],
            export_artifacts: vec![],
        };

        let error = apply_requested_post_processing(
            &mut bundle,
            &params,
            Some(&PostProcessingSpec {
                displacement: None,
                lithophane_attachments: vec![LithophaneAttachment {
                    id: "panel".to_string(),
                    enabled: true,
                    source: LithophaneAttachmentSource::File {
                        image_path: "/definitely/missing/lithophane.png".to_string(),
                    },
                    target_part_id: String::new(),
                    placement: LithophanePlacement {
                        mode: LithophanePlacementMode::PartSidePatch,
                        side: LithophaneSide::Front,
                        projection: ProjectionType::Planar,
                        width_mm: 40.0,
                        height_mm: 40.0,
                        offset_x_mm: 0.0,
                        offset_y_mm: 0.0,
                        rotation_deg: 0.0,
                        overflow_mode: OverflowMode::Contain,
                        bleed_margin_mm: 0.0,
                    },
                    relief: LithophaneRelief {
                        depth_mm: 1.0,
                        invert: false,
                    },
                    color: LithophaneColor {
                        mode: LithophaneColorMode::Cmyk,
                        channel_thickness_mm: 0.4,
                    },
                }],
            }),
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("Failed to open image for lithophane attachment"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn lithophane_attachment_target_part_id_must_exist_in_manifest() {
        let root =
            std::env::temp_dir().join(format!("ecky-litho-target-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let preview_stl_path = root.join("preview.stl");
        std::fs::write(
            &preview_stl_path,
            [&[0u8; 80][..], &0u32.to_le_bytes()[..]].concat(),
        )
        .unwrap();
        let manifest_path = root.join("manifest.json");
        std::fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&crate::models::ModelManifest {
                schema_version: 1,
                model_id: "model".to_string(),
                source_kind: crate::models::ModelSourceKind::Generated,
                engine_kind: crate::models::EngineKind::EckyIrV0,
                source_language: crate::models::SourceLanguage::EckyIrV0,
                geometry_backend: crate::models::GeometryBackend::EckyRust,
                document: crate::models::DocumentMetadata {
                    document_name: "doc".to_string(),
                    document_label: "doc".to_string(),
                    source_path: None,
                    object_count: 1,
                    warnings: vec![],
                },
                parts: vec![crate::models::PartBinding {
                    part_id: "body".to_string(),
                    freecad_object_name: "body".to_string(),
                    label: "Body".to_string(),
                    kind: "solid".to_string(),
                    semantic_role: None,
                    viewer_asset_path: None,
                    viewer_node_ids: vec![],
                    parameter_keys: vec![],
                    editable: true,
                    bounds: Some(crate::models::ManifestBounds {
                        x_min: -10.0,
                        y_min: -10.0,
                        z_min: 0.0,
                        x_max: 10.0,
                        y_max: 10.0,
                        z_max: 20.0,
                    }),
                    volume: None,
                    area: None,
                }],
                parameter_groups: vec![],
                control_primitives: vec![],
                control_relations: vec![],
                control_views: vec![],
                advisories: vec![],
                selection_targets: vec![],
                measurement_annotations: vec![],
                warnings: vec![],
                enrichment_state: crate::models::ManifestEnrichmentState {
                    status: crate::models::EnrichmentStatus::None,
                    proposals: vec![],
                },
            })
            .unwrap(),
        )
        .unwrap();
        let image_path = root.join("image.png");
        image::RgbImage::from_fn(2, 2, |_x, _y| image::Rgb([255, 255, 255]))
            .save(&image_path)
            .unwrap();

        let params = DesignParams::new();
        let mut bundle = crate::models::ArtifactBundle {
            schema_version: 1,
            model_id: "model".to_string(),
            source_kind: crate::models::ModelSourceKind::Generated,
            engine_kind: crate::models::EngineKind::EckyIrV0,
            source_language: crate::models::SourceLanguage::EckyIrV0,
            geometry_backend: crate::models::GeometryBackend::EckyRust,
            content_hash: "unchanged".to_string(),
            artifact_version: 1,
            fcstd_path: String::new(),
            manifest_path: manifest_path.to_string_lossy().to_string(),
            macro_path: None,
            preview_stl_path: preview_stl_path.to_string_lossy().to_string(),
            viewer_assets: vec![],
            edge_targets: vec![],
            face_targets: vec![],
            callout_anchors: vec![],
            measurement_guides: vec![],
            export_artifacts: vec![],
        };

        let error = apply_requested_post_processing(
            &mut bundle,
            &params,
            Some(&PostProcessingSpec {
                displacement: None,
                lithophane_attachments: vec![LithophaneAttachment {
                    id: "panel".to_string(),
                    enabled: true,
                    source: LithophaneAttachmentSource::File {
                        image_path: image_path.to_string_lossy().to_string(),
                    },
                    target_part_id: "missing".to_string(),
                    placement: LithophanePlacement {
                        mode: LithophanePlacementMode::PartSidePatch,
                        side: LithophaneSide::Front,
                        projection: ProjectionType::Planar,
                        width_mm: 20.0,
                        height_mm: 20.0,
                        offset_x_mm: 0.0,
                        offset_y_mm: 0.0,
                        rotation_deg: 0.0,
                        overflow_mode: OverflowMode::Contain,
                        bleed_margin_mm: 0.0,
                    },
                    relief: LithophaneRelief {
                        depth_mm: 1.0,
                        invert: false,
                    },
                    color: LithophaneColor {
                        mode: LithophaneColorMode::Mono,
                        channel_thickness_mm: 0.4,
                    },
                }],
            }),
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("references missing targetPartId 'missing'"),
            "unexpected error: {}",
            error
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ir_generated_bundle_supports_attachment_based_planar_cmyk_lithophane() {
        #[derive(Clone)]
        struct TestResolver {
            root: std::path::PathBuf,
        }

        impl crate::models::PathResolver for TestResolver {
            fn app_config_dir(&self) -> std::path::PathBuf {
                self.root.clone()
            }

            fn app_data_dir(&self) -> std::path::PathBuf {
                self.root.clone()
            }

            fn resource_path(&self, _path: &str) -> Option<std::path::PathBuf> {
                None
            }
        }

        let root =
            std::env::temp_dir().join(format!("ecky-ir-litho-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root: root.clone() };
        let mut bundle = crate::ecky_ir::render_model(
            r#"(model
                (part body
                  (extrude
                    (rounded_rect 32 32 4 12)
                    10)))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("ir render");

        let image_path = root.join("panel.png");
        image::RgbImage::from_fn(3, 3, |x, y| {
            if (x + y) % 2 == 0 {
                image::Rgb([255, 255, 255])
            } else {
                image::Rgb([32, 64, 255])
            }
        })
        .save(&image_path)
        .unwrap();

        apply_requested_post_processing(
            &mut bundle,
            &DesignParams::new(),
            Some(&PostProcessingSpec {
                displacement: None,
                lithophane_attachments: vec![LithophaneAttachment {
                    id: "panel".to_string(),
                    enabled: true,
                    source: LithophaneAttachmentSource::File {
                        image_path: image_path.to_string_lossy().to_string(),
                    },
                    target_part_id: "body".to_string(),
                    placement: LithophanePlacement {
                        mode: LithophanePlacementMode::PartSidePatch,
                        side: LithophaneSide::Front,
                        projection: ProjectionType::Planar,
                        width_mm: 24.0,
                        height_mm: 24.0,
                        offset_x_mm: 0.0,
                        offset_y_mm: 0.0,
                        rotation_deg: 0.0,
                        overflow_mode: OverflowMode::Contain,
                        bleed_margin_mm: 0.0,
                    },
                    relief: LithophaneRelief {
                        depth_mm: 1.0,
                        invert: false,
                    },
                    color: LithophaneColor {
                        mode: LithophaneColorMode::Cmyk,
                        channel_thickness_mm: 0.4,
                    },
                }],
            }),
        )
        .expect("post processing");

        assert!(std::path::Path::new(&bundle.preview_stl_path).exists());
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "3mf" && artifact.role == "primary"));
        std::fs::remove_dir_all(root).unwrap();
    }

    // ------------------------------------------------------------------
    // Phase 6 / 7 verification tests
    // ------------------------------------------------------------------

    /// Generic Ecky source uses config backend when request omits backend.
    #[test]
    fn ecky_source_uses_configured_backend_when_request_omits_backend() {
        assert_eq!(
            resolve_geometry_backend(&MacroDialect::EckyIrV0, None, GeometryBackend::Build123d),
            GeometryBackend::Build123d
        );
        assert_eq!(
            resolve_geometry_backend(&MacroDialect::EckyIrV0, None, GeometryBackend::Freecad),
            GeometryBackend::Freecad
        );
        assert_eq!(
            resolve_geometry_backend(
                &MacroDialect::EckyIrV0,
                Some(GeometryBackend::EckyRust),
                GeometryBackend::Build123d
            ),
            GeometryBackend::EckyRust,
        );
    }

    #[test]
    fn legacy_python_and_build123d_sources_keep_backend_defaults() {
        assert_eq!(
            resolve_geometry_backend(&MacroDialect::Build123d, None, GeometryBackend::Freecad),
            GeometryBackend::Build123d
        );
        assert_eq!(
            resolve_geometry_backend(
                &MacroDialect::CadFrameworkV1,
                None,
                GeometryBackend::Build123d
            ),
            GeometryBackend::Freecad
        );
    }

    #[test]
    fn ecky_rust_request_keeps_exact_only_source_on_ecky_rust_for_direct_probe() {
        let backend = resolve_dispatch_backend(
            r#"(model
                (part body
                  (sampled-radial-loft
                    (theta z fz)
                    :height 40
                    :z-steps 6
                    :theta-steps 24
                    :radius (+ 20 (* 2 (sin (+ (* theta 6) (* fz 3.141592653589793))))))))"#,
            &MacroDialect::EckyIrV0,
            GeometryBackend::EckyRust,
        )
        .expect("dispatch backend");

        assert_eq!(backend, GeometryBackend::EckyRust);
    }

    #[test]
    fn mixed_mesh_and_exact_only_ops_are_rejected_at_dispatch() {
        let err = resolve_dispatch_backend(
            r#"(model
                (part body
                  (union
                    (wall-pattern (:mode ribs :depth 0.4 :uFreq 8)
                      (extrude (circle 5) 18))
                    (sampled-radial-loft
                      (theta z fz)
                      :height 40
                      :z-steps 6
                      :theta-steps 24
                      :radius (+ 20 (* 2 (sin (+ (* theta 6) (* fz 3.141592653589793)))))))))"#,
            &MacroDialect::EckyIrV0,
            GeometryBackend::EckyRust,
        )
        .expect_err("mixed backend-exclusive ops must reject");

        assert!(err
            .to_string()
            .contains("cannot mix with exact-only ops like `sampled-radial-loft`"));
    }

    #[tokio::test]
    async fn ecky_rust_dispatch_falls_back_to_mesh_when_direct_occt_cannot_export_operation() {
        let root = temp_root("direct-fallback");
        let resolver = TestResolver { root: root.clone() };
        let state = test_state(&root);

        let bundle = render_model(
            r#"(model
                (part body
                  (wall-pattern (:mode ribs :depth 0.4 :uFreq 8)
                    (extrude (circle 5) 18))))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("mesh fallback render");

        assert!(bundle.model_id.starts_with("generated-ir-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(!bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_request_falls_forward_to_build123d_for_sampled_radial_loft() {
        let root = temp_root("eckyrust-sampled-radial-loft");
        let resolver = TestResolver { root: root.clone() };
        let direct_capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if direct_capability.available {
            let _ = std::fs::remove_dir_all(&root);
            return;
        }
        let capability = crate::runtime_capabilities::probe_build123d_runtime(&resolver);
        if !capability.available {
            let _ = std::fs::remove_dir_all(&root);
            return;
        }
        let state = test_state(&root);

        let bundle = render_model(
            r#"(model
                (part body
                  (sampled-radial-loft
                    (theta z fz)
                    :height 40
                    :z-steps 6
                    :theta-steps 24
                    :radius (+ 20 (* 2 (sin (+ (* theta 6) (* fz 3.141592653589793)))))
                    :z-map (+ z (* fz 2)))))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("build123d exact fallback render");

        assert_eq!(bundle.geometry_backend, GeometryBackend::Build123d);
        assert_eq!(
            bundle.source_language,
            crate::models::SourceLanguage::EckyIrV0
        );
        assert!(bundle.model_id.starts_with("generated-b123d-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(!bundle.edge_targets.is_empty());
        assert!(!bundle.face_targets.is_empty());

        let manifest = load_manifest_for_bundle(&bundle)
            .expect("load manifest")
            .expect("runtime manifest");
        assert!(manifest
            .selection_targets
            .iter()
            .any(|target| target.kind == crate::models::SelectionTargetKind::Edge));
        assert!(manifest
            .selection_targets
            .iter()
            .any(|target| target.kind == crate::models::SelectionTargetKind::Face));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn build123d_request_falls_back_to_mesh_for_wall_pattern_source() {
        let root = temp_root("build123d-wall-pattern");
        let resolver = TestResolver { root: root.clone() };
        let state = test_state(&root);

        let bundle = render_model(
            r#"(model
                (part body
                  (wall-pattern (:mode ribs :depth 0.4 :uFreq 8)
                    (extrude (circle 5) 18))))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::Build123d),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("build123d wall-pattern fallback render");

        assert_eq!(bundle.geometry_backend, GeometryBackend::EckyRust);
        assert!(bundle.model_id.starts_with("generated-ir-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(!bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn freecad_request_falls_back_to_mesh_for_wall_pattern_source() {
        let root = temp_root("freecad-wall-pattern");
        let resolver = TestResolver { root: root.clone() };
        let state = test_state(&root);

        let bundle = render_model(
            r#"(model
                (part body
                  (wall-pattern (:mode ribs :depth 0.4 :uFreq 8)
                    (extrude (circle 5) 18))))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::Freecad),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("freecad wall-pattern fallback render");

        assert_eq!(bundle.geometry_backend, GeometryBackend::EckyRust);
        assert!(bundle.model_id.starts_with("generated-ir-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(!bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_request_falls_forward_to_build123d_for_shell_sampled_radial_loft() {
        let root = temp_root("eckyrust-shell-sampled-radial-loft");
        let resolver = TestResolver { root: root.clone() };
        let direct_capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if direct_capability.available {
            let _ = std::fs::remove_dir_all(&root);
            return;
        }
        let capability = crate::runtime_capabilities::probe_build123d_runtime(&resolver);
        if !capability.available {
            let _ = std::fs::remove_dir_all(&root);
            return;
        }
        let state = test_state(&root);

        let bundle = render_model(
            r#"(model
                (part body
                  (shell 2
                    (sampled-radial-loft
                      (theta z fz)
                      :height 40
                      :z-steps 6
                      :theta-steps 24
                      :radius (+ 20 (* 2 (sin (+ (* theta 6) (* fz 3.141592653589793)))))
                      :z-map (+ z (* fz 2))))))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("build123d exact shell fallback render");

        assert_eq!(bundle.geometry_backend, GeometryBackend::Build123d);
        assert_eq!(
            bundle.source_language,
            crate::models::SourceLanguage::EckyIrV0
        );
        assert!(bundle.model_id.starts_with("generated-b123d-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(!bundle.edge_targets.is_empty());
        assert!(!bundle.face_targets.is_empty());

        let manifest = load_manifest_for_bundle(&bundle)
            .expect("load manifest")
            .expect("runtime manifest");
        assert!(manifest
            .selection_targets
            .iter()
            .any(|target| target.kind == crate::models::SelectionTargetKind::Edge));
        assert!(manifest
            .selection_targets
            .iter()
            .any(|target| target.kind == crate::models::SelectionTargetKind::Face));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_request_renders_dome_style_exact_stack_via_build123d() {
        let root = temp_root("eckyrust-dome-style-radial-loft");
        let resolver = TestResolver { root: root.clone() };
        let direct_capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if direct_capability.available {
            let _ = std::fs::remove_dir_all(&root);
            return;
        }
        let capability = crate::runtime_capabilities::probe_build123d_runtime(&resolver);
        if !capability.available {
            let _ = std::fs::remove_dir_all(&root);
            return;
        }
        let state = test_state(&root);

        let bundle = render_model(
            r#"(model
                (part body
                  (translate 0 0 8
                    (difference
                      (shell 2
                        (sampled-radial-loft
                          (theta z fz)
                          :height 40
                          :z-steps 8
                          :theta-steps 32
                          :radius (+ 18 (* 2 (sin (+ (* theta 6) (* fz 3.141592653589793)))))
                          :z-map (+ z (* fz 2))))
                      (translate 0 0 28 (cylinder 4 18 32))))))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("build123d dome-style exact render");

        assert_eq!(bundle.geometry_backend, GeometryBackend::Build123d);
        assert_eq!(
            bundle.source_language,
            crate::models::SourceLanguage::EckyIrV0
        );
        assert!(bundle.model_id.starts_with("generated-b123d-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(!bundle.edge_targets.is_empty());
        assert!(!bundle.face_targets.is_empty());

        let manifest = load_manifest_for_bundle(&bundle)
            .expect("load manifest")
            .expect("runtime manifest");
        assert!(manifest
            .selection_targets
            .iter()
            .any(|target| target.kind == crate::models::SelectionTargetKind::Edge));
        assert!(manifest
            .selection_targets
            .iter()
            .any(|target| target.kind == crate::models::SelectionTargetKind::Face));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_dispatch_uses_direct_occt_for_sampled_radial_loft_when_sdk_ready() {
        let root = temp_root("direct-sampled-radial-loft");
        let resolver = TestResolver { root: root.clone() };
        let capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if !capability.available {
            let _ = std::fs::remove_dir_all(root);
            return;
        }

        let state = test_state(&root);
        let bundle = render_model(
            r#"(model
                (part body
                  (sampled-radial-loft
                    (theta z fz)
                    :height 40
                    :z-steps 6
                    :theta-steps 24
                    :radius (+ 20 (* 2 (sin (+ (* theta 6) (* fz 3.141592653589793)))))
                    :z-map (+ z (* fz 2)))))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("direct OCCT sampled radial loft render");

        assert_eq!(bundle.geometry_backend, GeometryBackend::EckyRust);
        assert!(bundle.model_id.starts_with("generated-direct-occt-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"
                && std::path::Path::new(&artifact.path).is_file()));
        assert!(!bundle.edge_targets.is_empty());
        assert!(!bundle.face_targets.is_empty());

        let manifest = load_manifest_for_bundle(&bundle)
            .expect("load manifest")
            .expect("runtime manifest");
        assert_eq!(
            manifest
                .parts
                .iter()
                .map(|part| part.part_id.as_str())
                .collect::<Vec<_>>(),
            vec!["body"]
        );
        assert!(!manifest.selection_targets.is_empty());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_dispatch_uses_direct_occt_for_shell_sampled_radial_loft_when_sdk_ready() {
        let root = temp_root("direct-shell-sampled-radial-loft");
        let resolver = TestResolver { root: root.clone() };
        let capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if !capability.available {
            let _ = std::fs::remove_dir_all(root);
            return;
        }

        let state = test_state(&root);
        let bundle = render_model(
            r#"(model
                (part body
                  (shell 2
                    (sampled-radial-loft
                      (theta z fz)
                      :height 40
                      :z-steps 6
                      :theta-steps 24
                      :radius (+ 20 (* 2 (sin (+ (* theta 6) (* fz 3.141592653589793)))))
                      :z-map (+ z (* fz 2))))))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("direct OCCT sampled radial shell render");

        assert_eq!(bundle.geometry_backend, GeometryBackend::EckyRust);
        assert!(bundle.model_id.starts_with("generated-direct-occt-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"
                && std::path::Path::new(&artifact.path).is_file()));
        assert!(!bundle.edge_targets.is_empty());
        assert!(!bundle.face_targets.is_empty());

        let manifest = load_manifest_for_bundle(&bundle)
            .expect("load manifest")
            .expect("runtime manifest");
        assert_eq!(
            manifest
                .parts
                .iter()
                .map(|part| part.part_id.as_str())
                .collect::<Vec<_>>(),
            vec!["body"]
        );
        assert!(!manifest.selection_targets.is_empty());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_dispatch_applies_exact_edge_target_id_when_sdk_ready() {
        let root = temp_root("direct-exact-edge-target-id");
        let resolver = TestResolver { root: root.clone() };
        let capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if !capability.available {
            let _ = std::fs::remove_dir_all(root);
            return;
        }

        let state = test_state(&root);
        let base_bundle = render_model(
            r#"(model
                (part body (box 20 20 10)))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("base direct OCCT render");
        let edge_target_id = base_bundle
            .edge_targets
            .first()
            .and_then(|target| target.canonical_target_id.clone())
            .expect("box edge target");
        let drifted_edge_target_id = edge_target_id.replacen(":edge:0:", ":edge:999:", 1);
        assert_ne!(drifted_edge_target_id, edge_target_id);

        let exact_source = format!(
            r#"(model
                (part body
                  (fillet 1.5 :edges "target-id:{drifted_edge_target_id}" (box 20 20 10))))"#
        );
        let bundle = render_model(
            &exact_source,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("exact edge target-id direct OCCT render");

        assert_eq!(bundle.geometry_backend, GeometryBackend::EckyRust);
        assert!(bundle.model_id.starts_with("generated-direct-occt-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"
                && std::path::Path::new(&artifact.path).is_file()));
        assert!(
            edge_target_id.starts_with("body:edge:"),
            "unexpected edge target id: {edge_target_id}"
        );

        let manifest = load_manifest_for_bundle(&bundle)
            .expect("load manifest")
            .expect("runtime manifest");
        assert_eq!(
            manifest
                .parts
                .iter()
                .map(|part| part.part_id.as_str())
                .collect::<Vec<_>>(),
            vec!["body"]
        );
        assert!(!manifest.selection_targets.is_empty());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_dispatch_applies_exact_edge_alias_target_id_when_sdk_ready() {
        let root = temp_root("direct-exact-edge-alias-target-id");
        let resolver = TestResolver { root: root.clone() };
        let capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if !capability.available {
            let _ = std::fs::remove_dir_all(root);
            return;
        }

        let state = test_state(&root);
        let base_bundle = render_model(
            r#"(model
                (part body (box 20 20 10)))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("base direct OCCT render");
        let base_manifest = load_manifest_for_bundle(&base_bundle)
            .expect("load manifest")
            .expect("runtime manifest");
        let edge_alias_target_id = base_manifest
            .selection_targets
            .iter()
            .find(|target| target.kind == crate::models::SelectionTargetKind::Edge)
            .and_then(|target| target.canonical_target_id.clone())
            .expect("box edge alias target");

        let exact_source = format!(
            r#"(model
                (part body
                  (fillet 1.5 :edges "target-id:{edge_alias_target_id}" (box 20 20 10))))"#
        );
        let bundle = render_model(
            &exact_source,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("exact edge alias direct OCCT render");

        assert_eq!(bundle.geometry_backend, GeometryBackend::EckyRust);
        assert!(bundle.model_id.starts_with("generated-direct-occt-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"
                && std::path::Path::new(&artifact.path).is_file()));

        let manifest = load_manifest_for_bundle(&bundle)
            .expect("load manifest")
            .expect("runtime manifest");
        assert!(!manifest.selection_targets.is_empty());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_dispatch_applies_exact_face_target_id_for_shell_when_sdk_ready() {
        let root = temp_root("direct-exact-face-target-id");
        let resolver = TestResolver { root: root.clone() };
        let capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if !capability.available {
            let _ = std::fs::remove_dir_all(root);
            return;
        }

        let state = test_state(&root);
        let base_bundle = render_model(
            r#"(model
                (part body (box 20 20 10)))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("base direct OCCT render");
        let face_target_id = base_bundle
            .face_targets
            .first()
            .and_then(|target| target.canonical_target_id.clone())
            .expect("box face target");
        let drifted_face_target_id = face_target_id.replacen(":face:0:", ":face:999:", 1);
        assert_ne!(drifted_face_target_id, face_target_id);

        let exact_source = format!(
            r#"(model
                (part body
                  (shell 1.5 :faces "target-id:{drifted_face_target_id}" (box 20 20 10))))"#
        );
        let bundle = render_model(
            &exact_source,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("exact face target-id direct OCCT shell render");

        assert_eq!(bundle.geometry_backend, GeometryBackend::EckyRust);
        assert!(bundle.model_id.starts_with("generated-direct-occt-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"
                && std::path::Path::new(&artifact.path).is_file()));
        assert!(
            face_target_id.starts_with("body:face:"),
            "unexpected face target id: {face_target_id}"
        );

        let manifest = load_manifest_for_bundle(&bundle)
            .expect("load manifest")
            .expect("runtime manifest");
        assert_eq!(
            manifest
                .parts
                .iter()
                .map(|part| part.part_id.as_str())
                .collect::<Vec<_>>(),
            vec!["body"]
        );
        assert!(!manifest.selection_targets.is_empty());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_dispatch_applies_exact_face_alias_target_id_for_shell_when_sdk_ready() {
        let root = temp_root("direct-exact-face-alias-target-id");
        let resolver = TestResolver { root: root.clone() };
        let capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if !capability.available {
            let _ = std::fs::remove_dir_all(root);
            return;
        }

        let state = test_state(&root);
        let base_bundle = render_model(
            r#"(model
                (part body (box 20 20 10)))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("base direct OCCT render");
        let base_manifest = load_manifest_for_bundle(&base_bundle)
            .expect("load manifest")
            .expect("runtime manifest");
        let face_alias_target_id = base_manifest
            .selection_targets
            .iter()
            .find(|target| target.kind == crate::models::SelectionTargetKind::Face)
            .and_then(|target| target.canonical_target_id.clone())
            .expect("box face alias target");

        let exact_source = format!(
            r#"(model
                (part body
                  (shell 1.5 :faces "target-id:{face_alias_target_id}" (box 20 20 10))))"#
        );
        let bundle = render_model(
            &exact_source,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("exact face alias direct OCCT shell render");

        assert_eq!(bundle.geometry_backend, GeometryBackend::EckyRust);
        assert!(bundle.model_id.starts_with("generated-direct-occt-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"
                && std::path::Path::new(&artifact.path).is_file()));

        let manifest = load_manifest_for_bundle(&bundle)
            .expect("load manifest")
            .expect("runtime manifest");
        assert!(!manifest.selection_targets.is_empty());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_dispatch_renders_dome_style_exact_stack_via_direct_occt_when_sdk_ready() {
        let root = temp_root("direct-dome-style-radial-loft");
        let resolver = TestResolver { root: root.clone() };
        let capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if !capability.available {
            let _ = std::fs::remove_dir_all(root);
            return;
        }

        let state = test_state(&root);
        let bundle = render_model(
            r#"(model
                (part body
                  (translate 0 0 8
                    (difference
                      (shell 2
                        (sampled-radial-loft
                          (theta z fz)
                          :height 40
                          :z-steps 8
                          :theta-steps 32
                          :radius (+ 18 (* 2 (sin (+ (* theta 6) (* fz 3.141592653589793)))))
                          :z-map (+ z (* fz 2))))
                      (translate 0 0 28 (cylinder 4 18 32))))))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("direct OCCT dome-style exact render");

        assert_eq!(bundle.geometry_backend, GeometryBackend::EckyRust);
        assert!(bundle.model_id.starts_with("generated-direct-occt-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"
                && std::path::Path::new(&artifact.path).is_file()));
        assert!(!bundle.edge_targets.is_empty());
        assert!(!bundle.face_targets.is_empty());

        let manifest = load_manifest_for_bundle(&bundle)
            .expect("load manifest")
            .expect("runtime manifest");
        assert_eq!(
            manifest
                .parts
                .iter()
                .map(|part| part.part_id.as_str())
                .collect::<Vec<_>>(),
            vec!["body"]
        );
        assert!(!manifest.selection_targets.is_empty());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_dispatch_uses_direct_occt_step_when_sdk_ready() {
        let root = temp_root("direct-success");
        let resolver = TestResolver { root: root.clone() };
        let capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if !capability.available {
            let _ = std::fs::remove_dir_all(root);
            return;
        }

        let state = test_state(&root);
        let params = DesignParams::from([("width".to_string(), ParamValue::Number(24.0))]);
        let bundle = render_model(
            r#"(model
                (params (number width 10))
                (part body (extrude (rounded_rect width 12 2) 14)))"#,
            &params,
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("direct OCCT render");

        assert!(bundle.model_id.starts_with("generated-direct-occt-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"
                && std::path::Path::new(&artifact.path).is_file()));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_dispatch_uses_direct_occt_step_for_advanced_multi_part_when_sdk_ready() {
        let root = temp_root("direct-advanced");
        let resolver = TestResolver { root: root.clone() };
        let capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if !capability.available {
            let _ = std::fs::remove_dir_all(root);
            return;
        }

        let state = test_state(&root);
        let bundle = render_model(
            r#"(model
                (part base (fillet 0.6 (box 18 14 4)))
                (part shell (translate 28 0 0 (shell 0.8 (box 10 10 10))))
                (part lofted (translate -28 0 0 (loft 18 (circle 5) (rounded-rect 12 8 2))))
                (part pins (translate 0 -24 0 (grid-array 2 2 8 8 (cylinder 1.5 5)))))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("advanced direct OCCT render");

        assert!(bundle.model_id.starts_with("generated-direct-occt-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"
                && std::path::Path::new(&artifact.path).is_file()));

        let manifest = load_manifest_for_bundle(&bundle)
            .expect("load manifest")
            .expect("runtime manifest");
        assert_eq!(manifest.document.object_count, 4);
        assert_eq!(
            manifest
                .parts
                .iter()
                .map(|part| part.part_id.as_str())
                .collect::<Vec<_>>(),
            vec!["base", "shell", "lofted", "pins"]
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    /// Phase 7: post-processing is backend-agnostic.
    ///
    /// Render a model via the EckyRust backend, then override the bundle's
    /// `geometry_backend` to `Build123d` before running post-processing.
    /// The lithophane pipeline must produce the same 3MF output regardless of
    /// which backend generated the underlying geometry.
    #[test]
    fn post_processing_is_backend_agnostic_for_build123d_bundle() {
        #[derive(Clone)]
        struct TestResolver {
            root: std::path::PathBuf,
        }
        impl crate::models::PathResolver for TestResolver {
            fn app_config_dir(&self) -> std::path::PathBuf {
                self.root.clone()
            }
            fn app_data_dir(&self) -> std::path::PathBuf {
                self.root.clone()
            }
            fn resource_path(&self, _: &str) -> Option<std::path::PathBuf> {
                None
            }
        }

        let root = std::env::temp_dir().join(format!("ecky-phase7-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root: root.clone() };

        // Render via EckyRust to get a real bundle with actual geometry.
        let mut bundle = crate::ecky_ir::render_model(
            r#"(model (part body (extrude (rounded_rect 32 32 4 12) 10)))"#,
            &crate::models::DesignParams::new(),
            &resolver,
        )
        .expect("IR render");

        // Override the geometry_backend field to simulate a Build123d bundle.
        // This is the core of the Phase 7 invariant: post-processing must not
        // branch on the backend.
        bundle.geometry_backend = crate::models::GeometryBackend::Build123d;

        let image_path = root.join("panel.png");
        image::RgbImage::from_fn(3, 3, |x, y| {
            if (x + y) % 2 == 0 {
                image::Rgb([255u8, 255, 255])
            } else {
                image::Rgb([32, 64, 200])
            }
        })
        .save(&image_path)
        .unwrap();

        apply_requested_post_processing(
            &mut bundle,
            &crate::models::DesignParams::new(),
            Some(&PostProcessingSpec {
                displacement: None,
                lithophane_attachments: vec![LithophaneAttachment {
                    id: "panel".to_string(),
                    enabled: true,
                    source: LithophaneAttachmentSource::File {
                        image_path: image_path.to_string_lossy().to_string(),
                    },
                    target_part_id: "body".to_string(),
                    placement: LithophanePlacement {
                        mode: LithophanePlacementMode::PartSidePatch,
                        side: LithophaneSide::Front,
                        projection: ProjectionType::Planar,
                        width_mm: 24.0,
                        height_mm: 24.0,
                        offset_x_mm: 0.0,
                        offset_y_mm: 0.0,
                        rotation_deg: 0.0,
                        overflow_mode: OverflowMode::Contain,
                        bleed_margin_mm: 0.0,
                    },
                    relief: LithophaneRelief {
                        depth_mm: 1.0,
                        invert: false,
                    },
                    color: LithophaneColor {
                        mode: LithophaneColorMode::Cmyk,
                        channel_thickness_mm: 0.4,
                    },
                }],
            }),
        )
        .expect("post-processing must succeed on a Build123d-tagged bundle (Phase 7 invariant)");

        assert_eq!(
            bundle.geometry_backend,
            crate::models::GeometryBackend::Build123d,
            "geometry_backend must not be mutated by post-processing"
        );
        assert!(
            bundle
                .export_artifacts
                .iter()
                .any(|a| a.format == "3mf" && a.role == "primary"),
            "post-processing must produce a 3MF for a Build123d-tagged bundle"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
