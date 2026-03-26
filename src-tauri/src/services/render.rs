use crate::contracts::infer_macro_dialect_from_code;
use crate::freecad;
use crate::models::{
    AppError, AppResult, AppState, ArtifactBundle, DesignParams, MacroDialect, ModelManifest,
    PathResolver,
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
    bundle.export_artifacts.clear();

    if has_explicit_attachment_path && !post_proc.lithophane_attachments.is_empty() {
        let resolved_attachments =
            resolve_lithophane_attachments(bundle, parameters, &post_proc.lithophane_attachments)?;

        if !resolved_attachments.is_empty() {
            let export_dir = crate::lithophane::export_dir_for_preview(stl_path);
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
    post_processing: Option<&crate::contracts::PostProcessingSpec>,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    let _guard = state.render_lock.lock().await;
    let resolved_macro_dialect =
        macro_dialect.unwrap_or_else(|| infer_macro_dialect_from_code(macro_code));
    let mut result = if resolved_macro_dialect == MacroDialect::EckyIrV0 {
        crate::ecky_ir::render_model(macro_code, parameters, app)
    } else {
        freecad::render_model(
            macro_code,
            parameters,
            configured_freecad_cmd(state).as_deref(),
            app,
        )
    };
    if let Ok(ref mut bundle) = result {
        apply_requested_post_processing(bundle, parameters, post_processing)?;
        let runtime_cache_dir = freecad::runtime_cache_dir(app)?;
        freecad::evict_cache_if_needed(&runtime_cache_dir);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::apply_requested_post_processing;
    use crate::contracts::{
        DisplacementSpec, LithophaneAttachment, LithophaneAttachmentSource, LithophaneColor,
        LithophaneColorMode, LithophanePlacement, LithophanePlacementMode, LithophaneRelief,
        LithophaneSide, OverflowMode, PostProcessingSpec, ProjectionType,
    };
    use crate::models::DesignParams;

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
            content_hash: "unchanged".to_string(),
            artifact_version: 1,
            fcstd_path: "/tmp/model.FCStd".to_string(),
            manifest_path: "/tmp/missing-manifest.json".to_string(),
            macro_path: None,
            preview_stl_path: "/tmp/nonexistent-preview.stl".to_string(),
            viewer_assets: vec![],
            edge_targets: vec![],
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
            content_hash: "unchanged".to_string(),
            artifact_version: 1,
            fcstd_path: "/tmp/model.FCStd".to_string(),
            manifest_path: "/tmp/missing-manifest.json".to_string(),
            macro_path: None,
            preview_stl_path: preview_stl_path.to_string_lossy().to_string(),
            viewer_assets: vec![],
            edge_targets: vec![],
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
            content_hash: "unchanged".to_string(),
            artifact_version: 1,
            fcstd_path: String::new(),
            manifest_path: manifest_path.to_string_lossy().to_string(),
            macro_path: None,
            preview_stl_path: preview_stl_path.to_string_lossy().to_string(),
            viewer_assets: vec![],
            edge_targets: vec![],
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
}
