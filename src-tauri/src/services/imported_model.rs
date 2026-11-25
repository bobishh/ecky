use crate::contracts::{
    validate_model_runtime_bundle, Advisory, AdvisoryCondition, AdvisorySeverity, AppError,
    AppResult, ArtifactBundle, ControlPrimitive, ControlPrimitiveKind, ControlView,
    ControlViewScope, ControlViewSection, ControlViewSource, DesignOutput, DesignParams,
    EnrichmentStatus, FreecadLibraryImportRequest, FreecadLibraryItem, InteractionMode,
    MacroDialect, Message, MessageRole, MessageStatus, ModelManifest, ModelSourceKind, ParamValue,
    PrimitiveBinding, SourceLanguage, UiField, UiSpec,
};
use crate::models::{AppState, PathResolver};
use crate::services::render::configured_freecad_cmd;
use crate::services::render_snapshot::{build_render_snapshot, RenderSnapshotInput};
use crate::services::session::{build_saved_version_snapshot, write_last_snapshot};
use crate::{db, freecad};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[allow(clippy::large_enum_variant)]
pub enum ImportedModelSource {
    #[specta(rename_all = "camelCase")]
    Fcstd { source_path: String },
    #[specta(rename_all = "camelCase")]
    FreecadLibrary { item: FreecadLibraryItem },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportModelIntent {
    pub source: ImportedModelSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImportedModelProjection {
    pub thread_id: String,
    pub message_id: String,
    pub title: String,
    pub message: Message,
    pub design_output: DesignOutput,
    pub artifact_bundle: ArtifactBundle,
    pub model_manifest: ModelManifest,
    pub snapshot_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImportedParameterApplyRequest {
    pub thread_id: String,
    pub target_message_id: String,
    pub parameters: DesignParams,
    #[serde(default)]
    pub persist: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_name: Option<String>,
}

pub async fn apply_imported_parameters(
    input: ImportedParameterApplyRequest,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<crate::services::manual_parameters::ManualParameterApplyResponse> {
    let target = {
        let conn = state.db.lock().await;
        crate::services::target::resolve_target(
            &conn,
            app,
            Some(input.thread_id.clone()),
            Some(input.target_message_id.clone()),
        )?
    };
    let design = target
        .design
        .as_ref()
        .ok_or_else(|| AppError::validation("Imported target has no design output."))?;
    if !design.macro_code.trim().is_empty() {
        return Err(AppError::validation(
            "Imported parameter intent requires a source-less imported component target.",
        ));
    }
    let artifact_bundle = target.artifact_bundle.as_ref().ok_or_else(|| {
        AppError::validation("Imported parameter intent requires target artifact runtime.")
    })?;
    let model_manifest = target.model_manifest.as_ref().ok_or_else(|| {
        AppError::validation("Imported parameter intent requires target model manifest.")
    })?;
    if !matches!(
        artifact_bundle.source_kind,
        ModelSourceKind::ImportedFcstd
            | ModelSourceKind::ImportedStep
            | ModelSourceKind::ImportedMesh
    ) || !matches!(
        model_manifest.source_kind,
        ModelSourceKind::ImportedFcstd
            | ModelSourceKind::ImportedStep
            | ModelSourceKind::ImportedMesh
    ) {
        return Err(AppError::validation(
            "Imported parameter intent requires an imported component runtime.",
        ));
    }

    crate::services::manual_parameters::apply_manual_parameters(
        crate::services::manual_parameters::ManualParameterApplyRequest {
            thread_id: input.thread_id,
            target_message_id: input.target_message_id,
            parameters: input.parameters,
            persist: input.persist,
            title: input.title,
            version_name: input.version_name,
        },
        state,
        app,
    )
    .await
}

pub async fn import_model_intent(
    input: ImportModelIntent,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<ImportedModelProjection> {
    let _guard = state.acquire_geometry_render().await;
    let (bundle, source_fallback_title) = match &input.source {
        ImportedModelSource::Fcstd { source_path } => {
            let bundle =
                freecad::import_fcstd(source_path, configured_freecad_cmd(state).as_deref(), app)?;
            let fallback = Path::new(source_path)
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("Imported FreeCAD Model")
                .to_string();
            (bundle, fallback)
        }
        ImportedModelSource::FreecadLibrary { item } => {
            let request = FreecadLibraryImportRequest {
                item: item.clone(),
                thread_id: None,
                title: None,
            };
            let import_path = crate::freecad_library::import_path_from_request(&request)?;
            let source_path = import_path
                .to_str()
                .ok_or_else(|| AppError::internal("Invalid FreeCAD library import path."))?;
            let extension = import_path
                .extension()
                .and_then(|value| value.to_str())
                .map(str::to_ascii_lowercase)
                .unwrap_or_default();
            if !matches!(
                extension.as_str(),
                "fcstd" | "step" | "stp" | "stl" | "obj" | "3mf"
            ) {
                return Err(AppError::validation(format!(
                    "FreeCAD library format '{}' is not importable yet.",
                    extension
                )));
            }
            let bundle = if matches!(extension.as_str(), "stl" | "obj" | "3mf") {
                crate::freecad_library::import_mesh_from_request(&request, app)?
            } else {
                match extension.as_str() {
                    "fcstd" => freecad::import_fcstd(
                        source_path,
                        configured_freecad_cmd(state).as_deref(),
                        app,
                    )?,
                    "step" | "stp" => freecad::import_step(
                        source_path,
                        configured_freecad_cmd(state).as_deref(),
                        app,
                    )?,
                    _ => unreachable!("validated library extension"),
                }
            };
            (bundle, item.name.clone())
        }
    };

    let raw_manifest = crate::model_runtime::read_model_manifest(app, &bundle.model_id)?;
    let requested_title = input.title.or_else(|| {
        let label = raw_manifest.document.document_label.trim();
        let name = raw_manifest.document.document_name.trim();
        (label.is_empty() && name.is_empty()).then_some(source_fallback_title)
    });
    let result = persist_imported_runtime(
        input.thread_id,
        requested_title,
        bundle,
        raw_manifest,
        state,
        app,
    )
    .await?;
    let runtime_cache_dir = freecad::runtime_cache_dir(app)?;
    freecad::evict_cache_if_needed(&runtime_cache_dir);
    Ok(result)
}

pub async fn persist_imported_runtime(
    thread_id: Option<String>,
    requested_title: Option<String>,
    artifact_bundle: ArtifactBundle,
    model_manifest: ModelManifest,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<ImportedModelProjection> {
    if !matches!(
        model_manifest.source_kind,
        ModelSourceKind::ImportedFcstd
            | ModelSourceKind::ImportedStep
            | ModelSourceKind::ImportedMesh
    ) {
        return Err(AppError::validation(
            "Imported model intent requires an imported runtime bundle.",
        ));
    }

    let design_output = build_imported_output(&model_manifest, None);
    let semantic_manifest = ensure_initial_semantic_manifest(
        model_manifest,
        &design_output.ui_spec,
        &design_output.initial_params,
    )?;
    let (artifact_bundle, model_manifest) = crate::model_runtime::write_runtime_bundle(
        app,
        &artifact_bundle.model_id,
        &artifact_bundle,
        &semantic_manifest,
    )?;
    validate_model_runtime_bundle(&model_manifest, &artifact_bundle)?;
    let snapshot_id = build_render_snapshot(RenderSnapshotInput {
        design: &design_output,
        effective_params: &design_output.initial_params,
        artifact_bundle: &artifact_bundle,
        model_manifest: &model_manifest,
    })?
    .snapshot_id;

    let title = resolve_title(&model_manifest, requested_title.as_deref());
    let thread_id = thread_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let projects_root = state.config.lock().unwrap().projects_root.clone();
    let db = state.db.lock().await;
    let existing_title = db::get_thread_title(&db, &thread_id)
        .map_err(|error| AppError::persistence(error.to_string()))?;
    let traits = existing_title.is_none().then(crate::generate_genie_traits);
    let thread_title = existing_title.as_deref().unwrap_or(&title);
    let canonical_title = thread_title.to_string();
    db::create_or_update_thread(&db, &thread_id, thread_title, now, traits.as_ref())
        .map_err(|error| AppError::persistence(error.to_string()))?;

    if matches!(
        model_manifest.source_kind,
        ModelSourceKind::ImportedFcstd | ModelSourceKind::ImportedStep
    ) {
        let binding = crate::thread_source_binding::bind_new_thread(
            app,
            &db,
            projects_root.as_deref(),
            &thread_id,
            thread_title,
        )?;
        let source_path = model_manifest
            .document
            .source_path
            .as_deref()
            .ok_or_else(|| AppError::validation("Imported CAD manifest has no source path."))?;
        materialize_imported_cad_source(Path::new(&binding.folder_path), Path::new(source_path))?;
    }

    let parent_message_id = db::get_thread_head_version_id(&db, &thread_id)
        .ok()
        .flatten();
    let message_id = Uuid::new_v4().to_string();
    let message = Message {
        id: message_id.clone(),
        parent_message_id,
        role: MessageRole::Assistant,
        content: design_output.response.clone(),
        status: MessageStatus::Success,
        output: Some(design_output.clone()),
        usage: None,
        artifact_bundle: Some(artifact_bundle.clone()),
        model_manifest: Some(model_manifest.clone()),
        structural_verification: None,
        agent_origin: None,
        image_data: None,
        visual_kind: None,
        attachment_images: Vec::new(),
        timestamp: now,
    };
    db::add_message_checked(&db, &thread_id, &message)
        .map_err(|error| AppError::persistence(error.to_string()))?;
    let _ = crate::persist_thread_summary(&db, &thread_id, thread_title);
    drop(db);

    state
        .authoring_actor_registry
        .invalidate_authoring_actors_for_thread(&thread_id)
        .await;
    let snapshot = build_saved_version_snapshot(
        Some(design_output.clone()),
        thread_id.clone(),
        message_id.clone(),
        Some(artifact_bundle.clone()),
        Some(model_manifest.clone()),
        None,
    );
    {
        let mut latest = state.last_snapshot.lock().unwrap();
        *latest = Some(snapshot.clone());
    }
    write_last_snapshot(app, Some(&snapshot));

    Ok(ImportedModelProjection {
        thread_id,
        message_id,
        title: canonical_title,
        message,
        design_output,
        artifact_bundle,
        model_manifest,
        snapshot_id,
    })
}

pub fn build_imported_output(
    manifest: &ModelManifest,
    existing_output: Option<&DesignOutput>,
) -> DesignOutput {
    let ui_spec = build_imported_ui_spec(manifest);
    let existing_params = existing_output
        .map(|output| output.initial_params.clone())
        .unwrap_or_default();
    let initial_params = build_imported_params(manifest, &existing_params, &ui_spec);
    let is_mesh = manifest.source_kind == ModelSourceKind::ImportedMesh;
    DesignOutput {
        title: resolve_title(manifest, None),
        version_name: existing_output
            .map(|output| output.version_name.clone())
            .unwrap_or_else(|| "Imported".to_string()),
        response: if is_mesh {
            "Imported mesh reference.".to_string()
        } else {
            "Imported FreeCAD model.".to_string()
        },
        interaction_mode: InteractionMode::Design,
        macro_code: String::new(),
        macro_dialect: MacroDialect::Legacy,
        engine_kind: manifest.engine_kind,
        source_language: manifest.source_language,
        geometry_backend: manifest.geometry_backend,
        ui_spec,
        initial_params,
        post_processing: None,
    }
}

fn build_imported_ui_spec(manifest: &ModelManifest) -> UiSpec {
    let mut keys = BTreeSet::new();
    for group in &manifest.parameter_groups {
        if group.editable {
            keys.extend(group.parameter_keys.iter().cloned());
        }
    }
    for part in &manifest.parts {
        if part.editable {
            keys.extend(part.parameter_keys.iter().cloned());
        }
    }
    UiSpec {
        fields: keys
            .into_iter()
            .map(|key| UiField::Number {
                label: humanize(&key),
                key,
                min: Some(0.0),
                max: None,
                step: Some(1.0),
                min_from: None,
                max_from: None,
                frozen: false,
            })
            .collect(),
    }
}

fn build_imported_params(
    manifest: &ModelManifest,
    existing: &DesignParams,
    ui_spec: &UiSpec,
) -> DesignParams {
    let mut params = existing.clone();
    for field in &ui_spec.fields {
        if params.contains_key(field.key()) {
            continue;
        }
        let bounds = manifest
            .parts
            .iter()
            .find(|part| part.parameter_keys.iter().any(|key| key == field.key()))
            .and_then(|part| part.bounds.as_ref());
        let value = bounds.map_or(0.0, |bounds| {
            if field.key().ends_with("_height") {
                (bounds.z_max - bounds.z_min).max(0.0)
            } else if field.key().ends_with("_depth") {
                (bounds.y_max - bounds.y_min).max(0.0)
            } else {
                (bounds.x_max - bounds.x_min).max(0.0)
            }
        });
        params.insert(field.key().to_string(), ParamValue::Number(value));
    }
    params
}

fn ensure_initial_semantic_manifest(
    mut manifest: ModelManifest,
    ui_spec: &UiSpec,
    params: &DesignParams,
) -> AppResult<ModelManifest> {
    if manifest.source_language == SourceLanguage::EckyIrV0 || ui_spec.fields.is_empty() {
        manifest.validate()?;
        return Ok(manifest);
    }

    let primitives = ui_spec
        .fields
        .iter()
        .enumerate()
        .map(|(order, field)| {
            let part_ids = infer_part_ids(&manifest, field.key());
            let parts = manifest
                .parts
                .iter()
                .filter(|part| part_ids.contains(&part.part_id))
                .collect::<Vec<_>>();
            ControlPrimitive {
                primitive_id: format!("primitive-{}", slugify(field.key())),
                label: infer_primitive_label(field, &parts),
                kind: match field {
                    UiField::Checkbox { .. } => ControlPrimitiveKind::Toggle,
                    UiField::Select { .. } | UiField::Image { .. } => ControlPrimitiveKind::Choice,
                    UiField::Range { .. } | UiField::Number { .. } => ControlPrimitiveKind::Number,
                },
                source: ControlViewSource::Generated,
                part_ids,
                bindings: vec![PrimitiveBinding {
                    parameter_key: field.key().to_string(),
                    scale: 1.0,
                    offset: 0.0,
                    min: None,
                    max: None,
                }],
                editable: !field.frozen(),
                order: order as u32,
            }
        })
        .collect::<Vec<_>>();
    let views = build_generated_views(&manifest, &primitives, ui_spec);
    manifest.control_primitives = primitives;
    manifest.control_views = views;
    manifest.advisories = build_generated_advisories(&manifest, params);
    manifest.validate()?;
    Ok(manifest)
}

fn build_generated_views(
    manifest: &ModelManifest,
    primitives: &[ControlPrimitive],
    ui_spec: &UiSpec,
) -> Vec<ControlView> {
    let by_id = primitives
        .iter()
        .map(|primitive| (primitive.primitive_id.as_str(), primitive))
        .collect::<HashMap<_, _>>();
    let mut views = Vec::new();
    let global = ordered_primitive_ids(primitives.iter());
    if !global.is_empty() {
        views.push(ControlView {
            view_id: "view-model".to_string(),
            label: "Model".to_string(),
            scope: ControlViewScope::Global,
            part_ids: Vec::new(),
            primitive_ids: global.clone(),
            sections: build_sections(&global, &by_id, ui_spec),
            is_default: true,
            source: ControlViewSource::Generated,
            status: EnrichmentStatus::Accepted,
            order: 0,
        });
    }
    for (index, part) in manifest.parts.iter().enumerate() {
        let ids = ordered_primitive_ids(
            primitives
                .iter()
                .filter(|primitive| primitive.part_ids.contains(&part.part_id)),
        );
        if ids.is_empty() {
            continue;
        }
        views.push(ControlView {
            view_id: format!("view-{}", slugify(&part.part_id)),
            label: role_title(part.semantic_role.as_deref())
                .unwrap_or(&part.label)
                .to_string(),
            scope: ControlViewScope::Part,
            part_ids: vec![part.part_id.clone()],
            primitive_ids: ids.clone(),
            sections: build_sections(&ids, &by_id, ui_spec),
            is_default: false,
            source: ControlViewSource::Generated,
            status: EnrichmentStatus::Accepted,
            order: index as u32 + 1,
        });
    }
    views
}

fn ordered_primitive_ids<'a>(
    primitives: impl Iterator<Item = &'a ControlPrimitive>,
) -> Vec<String> {
    let mut primitives = primitives
        .filter(|primitive| primitive.editable)
        .collect::<Vec<_>>();
    primitives.sort_by(|left, right| {
        left.order
            .cmp(&right.order)
            .then_with(|| left.label.cmp(&right.label))
    });
    primitives
        .into_iter()
        .map(|primitive| primitive.primitive_id.clone())
        .collect()
}

fn build_sections(
    primitive_ids: &[String],
    by_id: &HashMap<&str, &ControlPrimitive>,
    ui_spec: &UiSpec,
) -> Vec<ControlViewSection> {
    let mut primary = Vec::new();
    let mut advanced = Vec::new();
    for primitive_id in primitive_ids {
        let Some(primitive) = by_id.get(primitive_id.as_str()) else {
            continue;
        };
        let field = primitive.bindings.first().and_then(|binding| {
            ui_spec
                .fields
                .iter()
                .find(|field| field.key() == binding.parameter_key)
        });
        if field.is_some_and(|field| is_primary(field, &primitive.label)) {
            primary.push(primitive_id.clone());
        } else {
            advanced.push(primitive_id.clone());
        }
    }
    let mut sections = Vec::new();
    if !primary.is_empty() {
        sections.push(ControlViewSection {
            section_id: "main".to_string(),
            label: "Main".to_string(),
            primitive_ids: primary,
            collapsed: false,
        });
    }
    if !advanced.is_empty() {
        sections.push(ControlViewSection {
            section_id: "advanced".to_string(),
            label: "Advanced".to_string(),
            primitive_ids: advanced,
            collapsed: false,
        });
    }
    sections
}

fn build_generated_advisories(manifest: &ModelManifest, params: &DesignParams) -> Vec<Advisory> {
    let mut advisories = Vec::new();
    for primitive in &manifest.control_primitives {
        let Some(binding) = primitive.bindings.first() else {
            continue;
        };
        let Some(ParamValue::Number(value)) = params.get(&binding.parameter_key) else {
            continue;
        };
        let signature =
            format!("{} {}", primitive.label, binding.parameter_key).to_ascii_lowercase();
        if signature.contains("thickness") && *value < 1.2 {
            advisories.push(Advisory {
                advisory_id: format!("advisory-{}-thin", slugify(&primitive.primitive_id)),
                label: "Thin wall".to_string(),
                severity: AdvisorySeverity::Warning,
                primitive_ids: vec![primitive.primitive_id.clone()],
                view_ids: Vec::new(),
                message: "Wall thickness is below the recommended print range.".to_string(),
                condition: AdvisoryCondition::Below,
                threshold: Some(1.2),
            });
        }
        if signature.contains("clearance") && *value < 0.6 {
            advisories.push(Advisory {
                advisory_id: format!("advisory-{}-clearance", slugify(&primitive.primitive_id)),
                label: "Low clearance".to_string(),
                severity: AdvisorySeverity::Warning,
                primitive_ids: vec![primitive.primitive_id.clone()],
                view_ids: Vec::new(),
                message: "Clearance is below the recommended fit range.".to_string(),
                condition: AdvisoryCondition::Below,
                threshold: Some(0.6),
            });
        }
    }
    if let Some(view) = manifest
        .control_views
        .iter()
        .find(|view| view.label == "Connector")
    {
        if view.primitive_ids.len() > 1 {
            advisories.push(Advisory {
                advisory_id: "advisory-connector-fit".to_string(),
                label: "Connector fit".to_string(),
                severity: AdvisorySeverity::Info,
                primitive_ids: view.primitive_ids.clone(),
                view_ids: vec![view.view_id.clone()],
                message: "Connector changes may require matching hole and clearance adjustments."
                    .to_string(),
                condition: AdvisoryCondition::Always,
                threshold: None,
            });
        }
    }
    advisories
}

fn infer_part_ids(manifest: &ModelManifest, key: &str) -> Vec<String> {
    let mut part_ids = BTreeSet::new();
    for group in &manifest.parameter_groups {
        if group
            .parameter_keys
            .iter()
            .any(|candidate| candidate == key)
        {
            part_ids.extend(group.part_ids.iter().cloned());
        }
    }
    for part in &manifest.parts {
        if part.parameter_keys.iter().any(|candidate| candidate == key) {
            part_ids.insert(part.part_id.clone());
        }
    }
    part_ids.into_iter().collect()
}

fn infer_primitive_label(field: &UiField, parts: &[&crate::contracts::PartBinding]) -> String {
    let base = if field.label().trim().is_empty() {
        humanize(field.key())
    } else {
        field.label().trim().to_string()
    };
    let Some(part) = parts.first() else {
        return base;
    };
    let role = role_title(part.semantic_role.as_deref()).unwrap_or("Part");
    let haystack = format!("{} {}", field.key(), base).to_ascii_lowercase();
    if haystack.contains(&role.to_ascii_lowercase())
        || [
            "connector",
            "hose",
            "spout",
            "lid",
            "cap",
            "handle",
            "body",
            "base",
        ]
        .iter()
        .any(|token| tokenize(&haystack).iter().any(|value| value == token))
    {
        base
    } else {
        format!("{} {}", role, base)
    }
}

fn is_primary(field: &UiField, label: &str) -> bool {
    if matches!(
        field,
        UiField::Checkbox { .. } | UiField::Select { .. } | UiField::Image { .. }
    ) {
        return true;
    }
    let tokens = tokenize(&format!("{} {}", field.key(), label));
    const ADVANCED: &[&str] = &[
        "resolution",
        "pattern",
        "sharpness",
        "frequency",
        "mix",
        "theta",
        "fade",
        "sample",
        "seed",
        "noise",
        "twist",
        "amplitude",
        "detail",
        "smoothing",
    ];
    if ADVANCED
        .iter()
        .any(|word| tokens.iter().any(|token| token == word))
    {
        return false;
    }
    const PRIMARY: &[&str] = &[
        "size",
        "diameter",
        "radius",
        "width",
        "height",
        "depth",
        "length",
        "thickness",
        "count",
        "clearance",
        "angle",
        "offset",
        "mesh",
        "logo",
        "connector",
        "hose",
        "spout",
        "handle",
        "lid",
        "cap",
    ];
    PRIMARY
        .iter()
        .any(|word| tokens.iter().any(|token| token == word))
}

fn resolve_title(manifest: &ModelManifest, requested: Option<&str>) -> String {
    requested
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            let label = manifest.document.document_label.trim();
            (!label.is_empty()).then_some(label)
        })
        .or_else(|| {
            let name = manifest.document.document_name.trim();
            (!name.is_empty()).then_some(name)
        })
        .unwrap_or(if manifest.source_kind == ModelSourceKind::ImportedMesh {
            "Imported Mesh Model"
        } else {
            "Imported FreeCAD Model"
        })
        .to_string()
}

fn materialize_imported_cad_source(folder: &Path, source: &Path) -> AppResult<PathBuf> {
    let file_name = source.file_name().ok_or_else(|| {
        AppError::validation(format!(
            "Imported CAD source '{}' has no file name.",
            source.display()
        ))
    })?;
    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if !matches!(extension.as_str(), "fcstd" | "step" | "stp") {
        return Err(AppError::validation(format!(
            "Imported CAD source '{}' is not FCStd or STEP.",
            source.display()
        )));
    }
    fs::create_dir_all(folder).map_err(|error| AppError::persistence(error.to_string()))?;
    let destination = folder.join(file_name);
    fs::copy(source, &destination).map_err(|error| {
        AppError::persistence(format!(
            "Failed to copy imported CAD source '{}' to '{}': {}",
            source.display(),
            destination.display(),
            error
        ))
    })?;
    Ok(destination)
}

fn humanize(value: &str) -> String {
    value
        .split(['_', '-', '.'])
        .filter(|token| !token.is_empty())
        .map(|token| {
            let mut chars = token.chars();
            chars.next().map_or_else(String::new, |first| {
                format!("{}{}", first.to_ascii_uppercase(), chars.as_str())
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn tokenize(value: &str) -> Vec<String> {
    value
        .to_ascii_lowercase()
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect()
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut separator = false;
    for character in value.trim().to_ascii_lowercase().chars() {
        if character.is_ascii_alphanumeric() {
            if separator && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(character);
            separator = false;
        } else {
            separator = true;
        }
    }
    slug
}

fn role_title(role: Option<&str>) -> Option<&'static str> {
    match role.unwrap_or("unknown") {
        "connector" => Some("Connector"),
        "lid" => Some("Lid"),
        "handle" => Some("Handle"),
        "body" => Some("Body"),
        "base" => Some("Base"),
        "ornament" => Some("Detail"),
        _ => None,
    }
}
