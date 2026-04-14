use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::contracts::{
    ArtifactBundle, ManifestBounds, ModelManifest, StructuralIssue, StructuralMetrics,
    StructuralVerificationResult, VerifierSource, VerifierStatus,
};

pub fn verify_structure(
    bundle: &ArtifactBundle,
    manifest: &ModelManifest,
) -> StructuralVerificationResult {
    let mut issues: Vec<StructuralIssue> = Vec::new();
    let mut preview_stl_size: Option<u64> = None;

    // 1. Preview STL exists and is non-empty
    let stl_path = Path::new(&bundle.preview_stl_path);
    match fs::metadata(stl_path) {
        Ok(meta) => {
            let size = meta.len();
            preview_stl_size = Some(size);
            if size == 0 {
                issues.push(StructuralIssue {
                    code: "PREVIEW_STL_EMPTY".into(),
                    message: "Preview STL file is empty (0 bytes).".into(),
                    part_id: None,
                    numeric_payload: Some(0.0),
                });
            }
        }
        Err(_) => {
            issues.push(StructuralIssue {
                code: "PREVIEW_STL_MISSING".into(),
                message: format!("Preview STL file not found: {}", bundle.preview_stl_path),
                part_id: None,
                numeric_payload: None,
            });
        }
    }

    // 2. Manifest parts non-empty
    if manifest.parts.is_empty() {
        issues.push(StructuralIssue {
            code: "MANIFEST_PARTS_EMPTY".into(),
            message: "Manifest contains no parts.".into(),
            part_id: None,
            numeric_payload: None,
        });
    }

    let part_ids: HashSet<&str> = manifest.parts.iter().map(|p| p.part_id.as_str()).collect();

    // 3. Per-part checks
    for part in &manifest.parts {
        // Viewer asset path exists
        if let Some(ref asset_path) = part.viewer_asset_path {
            if !Path::new(asset_path).exists() {
                issues.push(StructuralIssue {
                    code: "PART_ASSET_MISSING".into(),
                    message: format!(
                        "Part '{}' viewer asset not found: {}",
                        part.label, asset_path
                    ),
                    part_id: Some(part.part_id.clone()),
                    numeric_payload: None,
                });
            }
        }

        // Bounds finite and non-degenerate
        if let Some(ref bounds) = part.bounds {
            if !bounds_valid(bounds) {
                issues.push(StructuralIssue {
                    code: "BOUNDS_DEGENERATE".into(),
                    message: format!("Part '{}' has degenerate or non-finite bounds.", part.label),
                    part_id: Some(part.part_id.clone()),
                    numeric_payload: None,
                });
            }
        }

        // Volume positive
        if let Some(vol) = part.volume {
            if !vol.is_finite() || vol <= 0.0 {
                issues.push(StructuralIssue {
                    code: "VOLUME_NON_POSITIVE".into(),
                    message: format!("Part '{}' has non-positive volume: {}", part.label, vol),
                    part_id: Some(part.part_id.clone()),
                    numeric_payload: Some(vol),
                });
            }
        }

        // Area positive
        if let Some(area) = part.area {
            if !area.is_finite() || area <= 0.0 {
                issues.push(StructuralIssue {
                    code: "AREA_NON_POSITIVE".into(),
                    message: format!(
                        "Part '{}' has non-positive surface area: {}",
                        part.label, area
                    ),
                    part_id: Some(part.part_id.clone()),
                    numeric_payload: Some(area),
                });
            }
        }
    }

    // 4. Assembly-level spatial checks (requires bounds data)
    {
        let parts_with_bounds: Vec<&crate::contracts::PartBinding> = manifest
            .parts
            .iter()
            .filter(|p| p.bounds.is_some())
            .collect();

        // GROUND_CONTACT_MISSING: whole assembly z_min > 10mm
        if !parts_with_bounds.is_empty() {
            let assembly_z_min = parts_with_bounds
                .iter()
                .map(|p| p.bounds.as_ref().unwrap().z_min)
                .fold(f64::INFINITY, f64::min);
            if assembly_z_min.is_finite() && assembly_z_min > 10.0 {
                issues.push(StructuralIssue {
                    code: "GROUND_CONTACT_MISSING".into(),
                    message: format!(
                        "Assembly base is {:.1}mm above z=0 — model may not be grounded on the build plate.",
                        assembly_z_min
                    ),
                    part_id: None,
                    numeric_payload: Some(assembly_z_min),
                });
            }
        }

        // Multipart-only checks
        if parts_with_bounds.len() >= 2 {
            // Find primary part (largest by volume, or first if no volume data)
            let primary_idx = manifest
                .parts
                .iter()
                .enumerate()
                .filter(|(_, p)| p.bounds.is_some())
                .max_by(|(_, a), (_, b)| {
                    a.volume
                        .unwrap_or(0.0)
                        .partial_cmp(&b.volume.unwrap_or(0.0))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(i, _)| i)
                .unwrap_or(0);
            let max_volume = manifest
                .parts
                .iter()
                .filter_map(|p| p.volume)
                .fold(0.0_f64, f64::max);

            for (idx, part) in manifest.parts.iter().enumerate() {
                let Some(ref bounds) = part.bounds else {
                    continue;
                };
                if idx == primary_idx {
                    continue;
                } // skip primary

                // PART_DISCONNECTED: min AABB distance to all other parts > 25mm
                let min_dist = manifest
                    .parts
                    .iter()
                    .enumerate()
                    .filter(|(j, p)| *j != idx && p.bounds.is_some())
                    .map(|(_, other)| aabb_distance(bounds, other.bounds.as_ref().unwrap()))
                    .fold(f64::INFINITY, f64::min);

                if min_dist > 25.0 {
                    issues.push(StructuralIssue {
                        code: "PART_DISCONNECTED".into(),
                        message: format!(
                            "Part '{}' is spatially isolated — nearest part is {:.1}mm away.",
                            part.label, min_dist
                        ),
                        part_id: Some(part.part_id.clone()),
                        numeric_payload: Some(min_dist),
                    });
                }

                // PART_TOO_SMALL: volume < 0.5% of max AND < 500mm³
                if let Some(vol) = part.volume {
                    if max_volume > 0.0 && vol > 0.0 && vol / max_volume < 0.005 && vol < 500.0 {
                        issues.push(StructuralIssue {
                            code: "PART_TOO_SMALL".into(),
                            message: format!(
                                "Part '{}' volume ({:.2}mm³) is suspiciously small — may be a degenerate fragment.",
                                part.label, vol
                            ),
                            part_id: Some(part.part_id.clone()),
                            numeric_payload: Some(vol),
                        });
                    }
                }
            }
        }
    }

    // 5. Multipart consistency: viewer assets reference known parts
    for asset in &bundle.viewer_assets {
        if !part_ids.contains(asset.part_id.as_str()) {
            issues.push(StructuralIssue {
                code: "VIEWER_ASSET_ORPHAN".into(),
                message: format!(
                    "Viewer asset '{}' references unknown part_id '{}'.",
                    asset.label, asset.part_id
                ),
                part_id: Some(asset.part_id.clone()),
                numeric_payload: None,
            });
        }
    }

    // Collect metrics
    let mut total_volume: Option<f64> = None;
    let mut total_area: Option<f64> = None;
    let mut merged_bbox: Option<ManifestBounds> = None;

    for part in &manifest.parts {
        if let Some(vol) = part.volume {
            *total_volume.get_or_insert(0.0) += vol;
        }
        if let Some(area) = part.area {
            *total_area.get_or_insert(0.0) += area;
        }
        if let Some(ref b) = part.bounds {
            merged_bbox = Some(match merged_bbox {
                None => b.clone(),
                Some(m) => ManifestBounds {
                    x_min: m.x_min.min(b.x_min),
                    y_min: m.y_min.min(b.y_min),
                    z_min: m.z_min.min(b.z_min),
                    x_max: m.x_max.max(b.x_max),
                    y_max: m.y_max.max(b.y_max),
                    z_max: m.z_max.max(b.z_max),
                },
            });
        }
    }

    let passed = issues.is_empty();
    let summary = if passed {
        "All structural checks passed.".into()
    } else {
        let codes: Vec<&str> = issues.iter().map(|i| i.code.as_str()).collect();
        format!("Structural verification failed: {}", codes.join(", "))
    };

    StructuralVerificationResult {
        passed,
        summary,
        issues,
        metrics: StructuralMetrics {
            part_count: manifest.parts.len() as u32,
            preview_stl_size_bytes: preview_stl_size,
            total_volume,
            total_area,
            bbox: merged_bbox,
        },
        verifier_status: VerifierStatus::OkRustOnly,
        verifier_source: Some(VerifierSource::RustStructural),
    }
}

/// Minimum 3D distance between two axis-aligned bounding boxes.
/// Returns 0.0 when they overlap or touch.
fn aabb_distance(a: &ManifestBounds, b: &ManifestBounds) -> f64 {
    let dx = (a.x_min - b.x_max).max(b.x_min - a.x_max).max(0.0);
    let dy = (a.y_min - b.y_max).max(b.y_min - a.y_max).max(0.0);
    let dz = (a.z_min - b.z_max).max(b.z_min - a.z_max).max(0.0);
    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn bounds_valid(b: &ManifestBounds) -> bool {
    let vals = [b.x_min, b.y_min, b.z_min, b.x_max, b.y_max, b.z_max];
    if vals.iter().any(|v| !v.is_finite()) {
        return false;
    }
    // At least one axis must have min < max (non-degenerate)
    (b.x_min < b.x_max) || (b.y_min < b.y_max) || (b.z_min < b.z_max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::*;
    use std::io::Write;

    fn test_bundle(dir: &Path) -> ArtifactBundle {
        let stl_path = dir.join("preview.stl");
        // Write a minimal binary STL (84-byte header + 0 triangles)
        let mut f = fs::File::create(&stl_path).unwrap();
        f.write_all(&[0u8; 80]).unwrap(); // 80-byte header
        f.write_all(&0u32.to_le_bytes()).unwrap(); // 0 triangles
        f.flush().unwrap();

        let manifest_path = dir.join("manifest.json");
        fs::write(&manifest_path, "{}").unwrap();

        ArtifactBundle {
            schema_version: 1,
            model_id: "generated-test-001".into(),
            source_kind: ModelSourceKind::Generated,
            engine_kind: EngineKind::Freecad,
            source_language: SourceLanguage::LegacyPython,
            geometry_backend: GeometryBackend::Freecad,
            content_hash: "abc123".into(),
            artifact_version: 1,
            fcstd_path: dir.join("model.fcstd").to_string_lossy().into(),
            manifest_path: manifest_path.to_string_lossy().into(),
            macro_path: None,
            preview_stl_path: stl_path.to_string_lossy().into(),
            viewer_assets: vec![],
            edge_targets: vec![],
            callout_anchors: vec![],
            measurement_guides: vec![],
            export_artifacts: vec![],
        }
    }

    fn test_manifest() -> ModelManifest {
        ModelManifest {
            schema_version: 1,
            model_id: "generated-test-001".into(),
            source_kind: ModelSourceKind::Generated,
            engine_kind: EngineKind::Freecad,
            source_language: SourceLanguage::LegacyPython,
            geometry_backend: GeometryBackend::Freecad,
            document: DocumentMetadata {
                document_name: "Test".into(),
                document_label: "Test Model".into(),
                source_path: None,
                object_count: 1,
                warnings: vec![],
            },
            parts: vec![PartBinding {
                part_id: "part-1".into(),
                freecad_object_name: "Body".into(),
                label: "Main Body".into(),
                kind: "solid".into(),
                semantic_role: None,
                viewer_asset_path: None,
                viewer_node_ids: vec![],
                parameter_keys: vec![],
                editable: true,
                bounds: Some(ManifestBounds {
                    x_min: -10.0,
                    y_min: -10.0,
                    z_min: 0.0,
                    x_max: 10.0,
                    y_max: 10.0,
                    z_max: 20.0,
                }),
                volume: Some(1000.0),
                area: Some(600.0),
            }],
            parameter_groups: vec![],
            control_primitives: vec![],
            control_relations: vec![],
            control_views: vec![],
            advisories: vec![],
            selection_targets: vec![],
            measurement_annotations: vec![],
            warnings: vec![],
            enrichment_state: ManifestEnrichmentState {
                status: EnrichmentStatus::None,
                proposals: vec![],
            },
        }
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ecky-sv-test-{}-{}", std::process::id(), name));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn valid_bundle_passes() {
        let dir = temp_dir("valid");
        let bundle = test_bundle(&dir);
        let manifest = test_manifest();
        let result = verify_structure(&bundle, &manifest);
        assert!(result.passed, "Expected pass, got: {:?}", result.issues);
        assert_eq!(result.verifier_status, VerifierStatus::OkRustOnly);
        assert_eq!(result.verifier_source, Some(VerifierSource::RustStructural));
        assert_eq!(result.metrics.part_count, 1);
        assert!(result.metrics.preview_stl_size_bytes.unwrap() > 0);
        assert!((result.metrics.total_volume.unwrap() - 1000.0).abs() < f64::EPSILON);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_preview_stl_fails() {
        let dir = temp_dir("missing_stl");
        let mut bundle = test_bundle(&dir);
        bundle.preview_stl_path = dir.join("nonexistent.stl").to_string_lossy().into();
        let manifest = test_manifest();
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(result
            .issues
            .iter()
            .any(|i| i.code == "PREVIEW_STL_MISSING"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_preview_stl_fails() {
        let dir = temp_dir("empty_stl");
        let bundle = test_bundle(&dir);
        // Overwrite with empty file
        fs::write(&bundle.preview_stl_path, b"").unwrap();
        let manifest = test_manifest();
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(result.issues.iter().any(|i| i.code == "PREVIEW_STL_EMPTY"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_manifest_parts_fails() {
        let dir = temp_dir("empty_parts");
        let bundle = test_bundle(&dir);
        let mut manifest = test_manifest();
        manifest.parts.clear();
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(result
            .issues
            .iter()
            .any(|i| i.code == "MANIFEST_PARTS_EMPTY"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_part_asset_fails() {
        let dir = temp_dir("missing_asset");
        let bundle = test_bundle(&dir);
        let mut manifest = test_manifest();
        manifest.parts[0].viewer_asset_path =
            Some(dir.join("missing-part.stl").to_string_lossy().into());
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(result.issues.iter().any(|i| i.code == "PART_ASSET_MISSING"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn degenerate_bounds_fails() {
        let dir = temp_dir("degen_bounds");
        let bundle = test_bundle(&dir);
        let mut manifest = test_manifest();
        manifest.parts[0].bounds = Some(ManifestBounds {
            x_min: 5.0,
            y_min: 5.0,
            z_min: 5.0,
            x_max: 5.0, // all axes degenerate
            y_max: 5.0,
            z_max: 5.0,
        });
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(result.issues.iter().any(|i| i.code == "BOUNDS_DEGENERATE"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn non_positive_volume_fails() {
        let dir = temp_dir("neg_vol");
        let bundle = test_bundle(&dir);
        let mut manifest = test_manifest();
        manifest.parts[0].volume = Some(-5.0);
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(result
            .issues
            .iter()
            .any(|i| i.code == "VOLUME_NON_POSITIVE"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn non_positive_area_fails() {
        let dir = temp_dir("zero_area");
        let bundle = test_bundle(&dir);
        let mut manifest = test_manifest();
        manifest.parts[0].area = Some(0.0);
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(result.issues.iter().any(|i| i.code == "AREA_NON_POSITIVE"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn orphan_viewer_asset_fails() {
        let dir = temp_dir("orphan");
        let mut bundle = test_bundle(&dir);
        bundle.viewer_assets.push(ViewerAsset {
            part_id: "unknown-part".into(),
            node_id: "node-1".into(),
            object_name: "Ghost".into(),
            label: "Ghost Part".into(),
            path: dir.join("ghost.stl").to_string_lossy().into(),
            format: ViewerAssetFormat::Stl,
        });
        let manifest = test_manifest();
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(result
            .issues
            .iter()
            .any(|i| i.code == "VIEWER_ASSET_ORPHAN"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn nan_bounds_fails() {
        let dir = temp_dir("nan_bounds");
        let bundle = test_bundle(&dir);
        let mut manifest = test_manifest();
        manifest.parts[0].bounds = Some(ManifestBounds {
            x_min: f64::NAN,
            y_min: 0.0,
            z_min: 0.0,
            x_max: 10.0,
            y_max: 10.0,
            z_max: 10.0,
        });
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(result.issues.iter().any(|i| i.code == "BOUNDS_DEGENERATE"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn metrics_merge_multiple_parts() {
        let dir = temp_dir("merge");
        let bundle = test_bundle(&dir);
        let mut manifest = test_manifest();
        manifest.parts.push(PartBinding {
            part_id: "part-2".into(),
            freecad_object_name: "Body2".into(),
            label: "Second Part".into(),
            kind: "solid".into(),
            semantic_role: None,
            viewer_asset_path: None,
            viewer_node_ids: vec![],
            parameter_keys: vec![],
            editable: true,
            bounds: Some(ManifestBounds {
                x_min: 20.0,
                y_min: 20.0,
                z_min: 0.0,
                x_max: 30.0,
                y_max: 30.0,
                z_max: 15.0,
            }),
            volume: Some(500.0),
            area: Some(300.0),
        });
        let result = verify_structure(&bundle, &manifest);
        assert!(result.passed);
        assert_eq!(result.metrics.part_count, 2);
        assert!((result.metrics.total_volume.unwrap() - 1500.0).abs() < f64::EPSILON);
        assert!((result.metrics.total_area.unwrap() - 900.0).abs() < f64::EPSILON);
        let bbox = result.metrics.bbox.unwrap();
        assert!((bbox.x_min - (-10.0)).abs() < f64::EPSILON);
        assert!((bbox.x_max - 30.0).abs() < f64::EPSILON);
        fs::remove_dir_all(&dir).ok();
    }

    // ── Assembly-level checks ────────────────────────────────────────────────

    #[test]
    fn floating_assembly_triggers_ground_contact_missing() {
        let dir = temp_dir("gnd_miss");
        let bundle = test_bundle(&dir);
        let mut manifest = test_manifest();
        // Move the only part to z=50..70 — well above ground
        manifest.parts[0].bounds = Some(ManifestBounds {
            x_min: -10.0,
            y_min: -10.0,
            z_min: 50.0,
            x_max: 10.0,
            y_max: 10.0,
            z_max: 70.0,
        });
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(
            result
                .issues
                .iter()
                .any(|i| i.code == "GROUND_CONTACT_MISSING"),
            "expected GROUND_CONTACT_MISSING, got: {:?}",
            result.issues.iter().map(|i| &i.code).collect::<Vec<_>>()
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn grounded_assembly_passes_ground_contact() {
        let dir = temp_dir("gnd_ok");
        let bundle = test_bundle(&dir);
        let manifest = test_manifest(); // default z_min=0
        let result = verify_structure(&bundle, &manifest);
        assert!(result.passed);
        assert!(!result
            .issues
            .iter()
            .any(|i| i.code == "GROUND_CONTACT_MISSING"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn detached_secondary_part_triggers_part_disconnected() {
        let dir = temp_dir("disconn");
        let bundle = test_bundle(&dir);
        let mut manifest = test_manifest();
        // Primary part: x:-10..10, secondary 100mm away in X
        manifest.parts.push(PartBinding {
            part_id: "secondary".into(),
            freecad_object_name: "Secondary".into(),
            label: "Secondary".into(),
            kind: "solid".into(),
            semantic_role: None,
            viewer_asset_path: None,
            viewer_node_ids: vec![],
            parameter_keys: vec![],
            editable: true,
            bounds: Some(ManifestBounds {
                x_min: 100.0,
                y_min: -5.0,
                z_min: 0.0,
                x_max: 110.0,
                y_max: 5.0,
                z_max: 10.0,
            }),
            volume: Some(500.0),
            area: Some(100.0),
        });
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(
            result.issues.iter().any(|i| i.code == "PART_DISCONNECTED"),
            "expected PART_DISCONNECTED, got: {:?}",
            result.issues.iter().map(|i| &i.code).collect::<Vec<_>>()
        );
        assert!(result
            .issues
            .iter()
            .any(|i| i.code == "PART_DISCONNECTED" && i.part_id.as_deref() == Some("secondary")));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn adjacent_parts_do_not_trigger_disconnected() {
        let dir = temp_dir("adj_ok");
        let bundle = test_bundle(&dir);
        let mut manifest = test_manifest(); // primary: x:-10..10
        manifest.parts.push(PartBinding {
            part_id: "secondary".into(),
            freecad_object_name: "Secondary".into(),
            label: "Secondary".into(),
            kind: "solid".into(),
            semantic_role: None,
            viewer_asset_path: None,
            viewer_node_ids: vec![],
            parameter_keys: vec![],
            editable: true,
            bounds: Some(ManifestBounds {
                x_min: 10.0,
                y_min: -5.0,
                z_min: 0.0,
                x_max: 20.0,
                y_max: 5.0,
                z_max: 10.0,
            }),
            volume: Some(500.0),
            area: Some(100.0),
        });
        let result = verify_structure(&bundle, &manifest);
        assert!(
            !result.issues.iter().any(|i| i.code == "PART_DISCONNECTED"),
            "unexpected PART_DISCONNECTED: {:?}",
            result.issues
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn degenerate_tiny_secondary_triggers_part_too_small() {
        let dir = temp_dir("tiny");
        let bundle = test_bundle(&dir);
        let mut manifest = test_manifest(); // primary vol=1000
        manifest.parts.push(PartBinding {
            part_id: "tiny-part".into(),
            freecad_object_name: "Tiny".into(),
            label: "Tiny Part".into(),
            kind: "solid".into(),
            semantic_role: None,
            viewer_asset_path: None,
            viewer_node_ids: vec![],
            parameter_keys: vec![],
            editable: true,
            bounds: Some(ManifestBounds {
                x_min: 0.0,
                y_min: 0.0,
                z_min: 0.0,
                x_max: 1.0,
                y_max: 1.0,
                z_max: 1.0,
            }),
            volume: Some(0.5), // 0.05% of 1000, way below 0.5% threshold
            area: Some(6.0),
        });
        let result = verify_structure(&bundle, &manifest);
        assert!(!result.passed);
        assert!(
            result.issues.iter().any(|i| i.code == "PART_TOO_SMALL"),
            "expected PART_TOO_SMALL, got: {:?}",
            result.issues.iter().map(|i| &i.code).collect::<Vec<_>>()
        );
        assert!(result
            .issues
            .iter()
            .any(|i| i.code == "PART_TOO_SMALL" && i.part_id.as_deref() == Some("tiny-part")));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reasonable_secondary_part_passes_size_check() {
        let dir = temp_dir("size_ok");
        let bundle = test_bundle(&dir);
        let mut manifest = test_manifest(); // primary vol=1000
        manifest.parts.push(PartBinding {
            part_id: "secondary".into(),
            freecad_object_name: "Secondary".into(),
            label: "Secondary".into(),
            kind: "solid".into(),
            semantic_role: None,
            viewer_asset_path: None,
            viewer_node_ids: vec![],
            parameter_keys: vec![],
            editable: true,
            bounds: Some(ManifestBounds {
                x_min: 5.0,
                y_min: 5.0,
                z_min: 0.0,
                x_max: 10.0,
                y_max: 10.0,
                z_max: 10.0,
            }),
            volume: Some(100.0), // 10% of 1000 — fine
            area: Some(50.0),
        });
        let result = verify_structure(&bundle, &manifest);
        assert!(
            !result.issues.iter().any(|i| i.code == "PART_TOO_SMALL"),
            "unexpected PART_TOO_SMALL: {:?}",
            result.issues
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn single_part_model_skips_multipart_assembly_checks() {
        let dir = temp_dir("single");
        let bundle = test_bundle(&dir);
        let manifest = test_manifest(); // 1 part only
        let result = verify_structure(&bundle, &manifest);
        assert!(
            !result
                .issues
                .iter()
                .any(|i| i.code == "PART_DISCONNECTED" || i.code == "PART_TOO_SMALL"),
            "single-part model should not trigger multipart checks: {:?}",
            result.issues
        );
        fs::remove_dir_all(&dir).ok();
    }
}
