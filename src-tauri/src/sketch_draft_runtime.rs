use crate::models::SketchDocument;
use crate::models::{
    validate_component_package, validate_design_params, validate_sketch_definition,
    validate_ui_spec, AppError, AppResult, ArtifactBundle, ComponentDefinition, ComponentPackage,
    DesignParams, ExportArtifact, GeometryBackend, MacroDialect, PackageVisibility, PathResolver,
    SketchAcceptedBrepCandidateSource, SketchAcceptedBrepComponentPackageRequest,
    SketchBrepCandidateAcceptRequest, SketchBrepCandidateCell, SketchBrepCandidateEdge,
    SketchBrepCandidateGraph, SketchBrepCandidateRequest, SketchBrepCandidateResponse,
    SketchBrepCandidateSearch, SketchBrepCandidateSolution, SketchBrepCandidateSourceStrategy,
    SketchBrepCandidateVertex, SketchBrepProjectionValidation, SketchDefinition,
    SketchDraftOperationKind, SketchDraftRequest, SketchDraftSource, SketchFeatureSuggestion,
    SketchPreviewHullRequest, SketchPrimitive, SketchPrimitiveKind, SketchSuggestionRequest,
    SketchSuggestionResponse, SketchValidationIssue, SketchValidationSeverity, SketchView,
    SourceLanguage, COMPONENT_PACKAGE_SCHEMA_VERSION,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path};

const DEFAULT_EXTRUDE_AMOUNT: f64 = 12.0;
const DIMENSION_TOLERANCE: f64 = 1e-6;
const MAX_SOURCE_MAP_JSON_BYTES: usize = 96 * 1024;
const MAX_SOURCE_PRIMITIVE_POINTS: usize = 512;

pub fn generate_sketch_draft_source(request: SketchDraftRequest) -> AppResult<SketchDraftSource> {
    validate_sketch_definition("draft", &request.sketch)?;
    if !request.amount.is_finite() || request.amount <= 0.0 {
        return Err(AppError::validation(
            "sketch draft amount must be positive and finite.",
        ));
    }
    let part_id = ecky_symbol(&request.part_id, "partId")?;
    let mut warnings = Vec::new();
    let compact_sketch = compact_sketch_for_source(&request.sketch, &mut warnings);
    let sketch_expr = sketch_expr(&compact_sketch)?;
    let operation_expr = match request.operation {
        SketchDraftOperationKind::Extrude => {
            let symmetric = if request.symmetric {
                " :symmetric #t"
            } else {
                ""
            };
            format!(
                "(extrude\n      {}\n      {}{})",
                sketch_expr,
                format_number(request.amount),
                symmetric
            )
        }
        SketchDraftOperationKind::Revolve => format!(
            "(revolve\n      {}\n      {})",
            sketch_expr,
            format_number(request.amount)
        ),
    };

    let source_map_comment =
        sketch_document_source_map_comment(&request.part_id, &compact_sketch, &mut warnings)?;

    Ok(SketchDraftSource {
        source_language: SourceLanguage::EckyIrV0,
        geometry_backend: GeometryBackend::EckyRust,
        macro_dialect: MacroDialect::EckyIrV0,
        source: format!(
            "{}(model\n  (part {}\n    {}))\n",
            source_map_comment, part_id, operation_expr
        ),
        warnings,
    })
}

pub fn generate_sketch_draft_preview(
    request: SketchDraftRequest,
    app: &dyn PathResolver,
) -> AppResult<(SketchDraftSource, ArtifactBundle)> {
    let draft = generate_sketch_draft_source(request)?;
    let params = DesignParams::new();
    let bundle = crate::ecky_ir::render_model(&draft.source, &params, app)?;
    Ok((draft, bundle))
}

pub fn generate_sketch_preview_hull_source(
    request: SketchPreviewHullRequest,
) -> AppResult<SketchDraftSource> {
    if !request.fallback_depth.is_finite() || request.fallback_depth <= 0.0 {
        return Err(AppError::validation(
            "sketch preview hull fallbackDepth must be positive and finite.",
        ));
    }

    for sketch in &request.document.sketches {
        validate_sketch_definition("preview hull", sketch)?;
    }

    let part_id = ecky_symbol(&request.part_id, "partId")?;
    let profiles = resolve_preview_hull_profiles(&request)?;
    let search_views = CandidateViews::new(
        profiles.front_profiles.clone(),
        profiles.top.clone(),
        profiles.side.clone(),
    );
    let candidate_search = build_candidate_search(&search_views);
    let search_expr = candidate_cells_expr(&candidate_search.cells);
    let mut operands = Vec::new();

    let front_expr = front_profile_expr(&profiles.front_profiles)?;
    operands.push(format!(
        "(translate 0 0 {}\n        (extrude\n          {}\n          {}))",
        format_number(profiles.depth_min),
        front_expr,
        format_number(profiles.depth)
    ));

    if let Some(top) = &profiles.top {
        let top_expr = primitive_expr(&top.primitive)?;
        operands.push(format!(
            "(translate 0 {} 0\n        (rotate 90 0 0\n          (extrude\n            {}\n            {} :symmetric #t)))",
            format_number(profiles.front.bounds.center_y()),
            top_expr,
            format_number(profiles.front.bounds.height)
        ));
    }

    if let Some(side) = &profiles.side {
        let side_expr = primitive_expr(&side.primitive)?;
        operands.push(format!(
            "(translate {} 0 0\n        (rotate 0 -90 0\n          (extrude\n            {}\n            {} :symmetric #t)))",
            format_number(profiles.front.bounds.center_x()),
            side_expr,
            format_number(profiles.front.bounds.width)
        ));
    }

    let (hull_expr, source_kind) = if let Some(expr) = search_expr {
        (expr, "candidate cell search")
    } else if operands.len() == 1 {
        (operands.remove(0), "silhouettes")
    } else {
        (
            format!("(intersection\n      {})", operands.join("\n      ")),
            "silhouettes",
        )
    };
    let source_map_comment = sketch_document_source_map_comment_from_document(&request.document)?;
    let view_label = profiles.view_label();

    Ok(SketchDraftSource {
        source_language: SourceLanguage::EckyIrV0,
        geometry_backend: GeometryBackend::EckyRust,
        macro_dialect: MacroDialect::EckyIrV0,
        source: format!(
            "{}(model\n  (part {}\n    {}))\n",
            source_map_comment, part_id, hull_expr
        ),
        warnings: vec![format!(
            "preview hull from {} {}; not accepted BRep.",
            view_label, source_kind
        )],
    })
}

pub fn generate_sketch_preview_hull(
    request: SketchPreviewHullRequest,
    app: &dyn PathResolver,
) -> AppResult<(SketchDraftSource, ArtifactBundle)> {
    let draft = generate_sketch_preview_hull_source(request)?;
    let params = DesignParams::new();
    let bundle = crate::ecky_ir::render_model(&draft.source, &params, app)?;
    Ok((draft, bundle))
}

pub fn analyze_sketch_brep_candidates(
    request: SketchBrepCandidateRequest,
) -> AppResult<SketchBrepCandidateResponse> {
    for sketch in &request.document.sketches {
        validate_sketch_definition("brep candidates", sketch)?;
    }

    let front_profiles = find_closed_profiles(&request.document, SketchView::Front);
    if front_profiles.is_empty() {
        return Err(AppError::validation(
            "BRep candidate graph requires a closed Front profile.",
        ));
    }
    let top = find_closed_profile(&request.document, SketchView::Top);
    let side = find_closed_profile(&request.document, SketchView::Side);

    if top.is_none() && side.is_none() {
        return Err(AppError::validation(
            "BRep candidate graph requires a closed Top or Side profile.",
        ));
    }

    let views = CandidateViews::new(front_profiles, top, side);
    let vertices = build_candidate_vertices(&views);
    let search = build_candidate_search(&views);
    let edges = build_candidate_edges(&views, &vertices);
    let mut validation = validate_candidate_reprojection(&views, &edges);
    if !validation.passed && has_front_profile_prism_solution(&search) {
        validation = validate_front_profile_prism_solution(&views);
    }

    Ok(SketchBrepCandidateResponse {
        graph: SketchBrepCandidateGraph { vertices, edges },
        search,
        validation,
    })
}

pub fn generate_accepted_brep_candidate_source(
    request: SketchBrepCandidateAcceptRequest,
) -> AppResult<SketchAcceptedBrepCandidateSource> {
    let part_id = ecky_symbol(&request.part_id, "partId")?;
    let candidate_response = analyze_sketch_brep_candidates(SketchBrepCandidateRequest {
        document: request.document.clone(),
    })?;
    let accepted_solution = candidate_response
        .search
        .solutions
        .iter()
        .find(|solution| solution.solution_id == request.solution_id)
        .cloned()
        .ok_or_else(|| {
            AppError::validation(format!(
                "Accepted BRep candidate solution '{}' was not found.",
                request.solution_id
            ))
        })?;
    if !candidate_response.validation.passed {
        let detail = candidate_response
            .validation
            .issues
            .first()
            .map(|issue| issue.message.as_str())
            .or_else(|| {
                candidate_response
                    .validation
                    .evidence
                    .first()
                    .map(String::as_str)
            })
            .unwrap_or("candidate reprojection failed");
        return Err(AppError::validation(format!(
            "Accepted BRep candidate solution '{}' cannot be accepted: {}",
            request.solution_id, detail
        )));
    }
    if accepted_solution.cell_ids.is_empty() {
        return Err(AppError::validation(format!(
            "Accepted BRep candidate solution '{}' contains no cells.",
            request.solution_id
        )));
    }

    let cells_by_id = candidate_response
        .search
        .cells
        .iter()
        .map(|cell| (cell.cell_id.as_str(), cell))
        .collect::<HashMap<_, _>>();
    let mut selected_cells = Vec::new();
    for cell_id in &accepted_solution.cell_ids {
        let Some(cell) = cells_by_id.get(cell_id.as_str()) else {
            return Err(AppError::validation(format!(
                "Accepted BRep candidate solution '{}' references unknown cellId '{}'.",
                request.solution_id, cell_id
            )));
        };
        selected_cells.push((*cell).clone());
    }

    let (source_expr, strategy_evidence) =
        accepted_candidate_solution_expr(&request.document, &accepted_solution, &selected_cells)?;
    let source_map_comment = sketch_document_source_map_comment_from_document(&request.document)?;
    let accepted_comment = format!(
        "; ecky-accepted-brep-candidate-solution: {}\n",
        request.solution_id
    );
    let source = format!(
        "{}{}(model\n  (part {}\n    {}))\n",
        source_map_comment, accepted_comment, part_id, source_expr
    );
    let cell_label = if selected_cells.len() == 1 {
        "cell"
    } else {
        "cells"
    };
    let mut evidence = vec![format!(
        "accepted BRep candidate solution '{}' with {} {}",
        accepted_solution.solution_id,
        selected_cells.len(),
        cell_label
    )];
    evidence.extend(strategy_evidence);
    evidence.extend(candidate_response.validation.evidence.clone());
    evidence.extend(accepted_solution.evidence.clone());

    Ok(SketchAcceptedBrepCandidateSource {
        draft_source: SketchDraftSource {
            source_language: SourceLanguage::EckyIrV0,
            geometry_backend: GeometryBackend::EckyRust,
            macro_dialect: MacroDialect::EckyIrV0,
            source,
            warnings: Vec::new(),
        },
        candidate_response,
        accepted_solution,
        evidence,
    })
}

pub fn require_step_export_artifact(bundle: &ArtifactBundle) -> AppResult<&ExportArtifact> {
    let artifact = bundle
        .export_artifacts
        .iter()
        .find(|artifact| {
            artifact.format.eq_ignore_ascii_case("step") && !artifact.path.trim().is_empty()
        })
        .ok_or_else(|| {
            AppError::validation(
                "Accepted BRep candidate requires a STEP export artifact; mesh preview fallback is not CAD acceptance.",
            )
        })?;
    if !Path::new(artifact.path.trim()).is_file() {
        return Err(AppError::not_found(format!(
            "STEP export artifact for accepted BRep candidate was not found at '{}'.",
            artifact.path
        )));
    }
    Ok(artifact)
}

pub fn accepted_brep_candidate_to_component_package(
    request: SketchAcceptedBrepComponentPackageRequest,
) -> AppResult<ComponentPackage> {
    if request.source_ref.trim().is_empty() {
        return Err(AppError::validation(
            "Accepted BRep component package requires a non-empty sourceRef.",
        ));
    }
    if request.ports.is_empty() {
        return Err(AppError::validation(
            "Accepted BRep component package requires at least one explicit accepted port.",
        ));
    }

    let _accepted = generate_accepted_brep_candidate_source(SketchBrepCandidateAcceptRequest {
        part_id: request.component_id.clone(),
        document: request.document.clone(),
        solution_id: request.solution_id.clone(),
        tolerance: None,
    })?;
    let known_port_type_ids = request
        .port_types
        .iter()
        .map(|port_type| port_type.type_id.as_str())
        .collect::<HashSet<_>>();
    for port in &request.ports {
        if !known_port_type_ids.contains(port.type_id.as_str()) {
            return Err(AppError::validation(format!(
                "Accepted BRep component port '{}' references unknown port typeId '{}'.",
                port.port_id, port.type_id
            )));
        }
    }
    let accepted_target_ids = request
        .artifact_bundle
        .as_ref()
        .map(accepted_brep_target_ids);
    for port in &request.ports {
        if port.target_ids.is_empty() {
            continue;
        }
        let Some(target_ids) = accepted_target_ids.as_ref() else {
            return Err(AppError::validation(format!(
                "Accepted BRep component port '{}' uses targetIds but no accepted artifactBundle was provided.",
                port.port_id
            )));
        };
        for target_id in &port.target_ids {
            if !target_ids.contains(target_id) {
                return Err(AppError::validation(format!(
                    "Accepted BRep component port '{}' references unknown accepted BRep targetId '{}'.",
                    port.port_id, target_id
                )));
            }
        }
    }
    if !request.ui_spec.fields.is_empty() || !request.initial_params.is_empty() {
        validate_ui_spec(&request.ui_spec)?;
        validate_design_params(&request.initial_params, &request.ui_spec)?;
    }
    let params = if request.params.is_empty() {
        crate::component_package_runtime::component_params_from_ui_contract(
            &request.ui_spec,
            &request.initial_params,
        )
    } else {
        request.params.clone()
    };

    let package = ComponentPackage {
        schema_version: COMPONENT_PACKAGE_SCHEMA_VERSION,
        package_id: request.package_id,
        version: request.version,
        display_name: request.display_name,
        visibility: PackageVisibility::Source,
        tags: request.tags,
        port_types: request.port_types,
        mate_types: Vec::new(),
        components: vec![ComponentDefinition {
            component_id: request.component_id,
            version: request.component_version,
            display_name: request.component_display_name,
            source_ref: Some(request.source_ref),
            source_language: None,
            geometry_backend: None,
            macro_dialect: None,
            sketches: request.document.sketches,
            keepouts: Vec::new(),
            fusion_zones: Vec::new(),
            params,
            ui_spec: request.ui_spec,
            initial_params: request.initial_params,
            ports: request.ports,
        }],
        assemblies: Vec::new(),
    };
    validate_component_package(&package)?;
    Ok(package)
}

pub fn write_accepted_brep_component_package_project(
    project_dir: &Path,
    request: SketchAcceptedBrepComponentPackageRequest,
) -> AppResult<ComponentPackage> {
    let bundle = request.artifact_bundle.as_ref().ok_or_else(|| {
        AppError::validation(
            "Accepted BRep portable component package requires accepted artifactBundle.",
        )
    })?;
    let step_artifact = require_step_export_artifact(bundle)?.clone();
    let source_ref = accepted_brep_package_source_ref(
        &request.component_id,
        &request.source_ref,
        &step_artifact.path,
    );

    let mut package_request = request;
    package_request.source_ref = source_ref.clone();
    let package = accepted_brep_candidate_to_component_package(package_request)?;

    let source_path = project_dir.join(&source_ref);
    if let Some(parent) = source_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            AppError::persistence(format!(
                "Failed to create accepted BRep component source directory '{}': {}",
                parent.display(),
                err
            ))
        })?;
    }
    fs::copy(step_artifact.path.trim(), &source_path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to copy accepted BRep STEP source from '{}' to '{}': {}",
            step_artifact.path,
            source_path.display(),
            err
        ))
    })?;
    crate::component_package_runtime::write_component_package_manifest(project_dir, &package)?;
    Ok(package)
}

fn accepted_brep_target_ids(bundle: &ArtifactBundle) -> HashSet<String> {
    bundle
        .edge_targets
        .iter()
        .map(|target| target.target_id.clone())
        .chain(
            bundle
                .face_targets
                .iter()
                .map(|target| target.target_id.clone()),
        )
        .collect()
}

fn accepted_brep_package_source_ref(
    component_id: &str,
    requested_source_ref: &str,
    step_artifact_path: &str,
) -> String {
    normalize_safe_package_relative_path(requested_source_ref)
        .unwrap_or_else(|| default_accepted_brep_source_ref(component_id, step_artifact_path))
}

fn normalize_safe_package_relative_path(path: &str) -> Option<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut normalized = Vec::new();
    for component in Path::new(trimmed).components() {
        match component {
            Component::Normal(part) => normalized.push(part.to_string_lossy().to_string()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!normalized.is_empty()).then(|| normalized.join("/"))
}

fn default_accepted_brep_source_ref(component_id: &str, step_artifact_path: &str) -> String {
    let component_segment = sanitize_package_path_segment(component_id);
    let extension = Path::new(step_artifact_path.trim())
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::trim)
        .filter(|ext| !ext.is_empty())
        .map(|ext| ext.to_ascii_lowercase())
        .unwrap_or_else(|| "step".to_string());
    format!("artifacts/{component_segment}/model.{extension}")
}

fn sanitize_package_path_segment(value: &str) -> String {
    let mut output = String::new();
    let mut last_was_dash = false;
    for ch in value.chars() {
        let normalized = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else if matches!(ch, '.' | '_' | '-') {
            ch
        } else {
            '-'
        };
        if normalized == '-' {
            if output.is_empty() || last_was_dash {
                continue;
            }
            last_was_dash = true;
            output.push('-');
        } else {
            last_was_dash = false;
            output.push(normalized);
        }
    }
    let trimmed = output.trim_matches('-');
    if trimmed.is_empty() {
        "accepted-brep".to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn suggest_sketch_features(request: SketchSuggestionRequest) -> SketchSuggestionResponse {
    let mut candidates = Vec::new();
    let mut warnings = Vec::new();

    for sketch in request
        .document
        .sketches
        .iter()
        .filter(|sketch| sketch.view == SketchView::Front)
    {
        for primitive in &sketch.primitives {
            match profile_measure(primitive) {
                ProfileMeasure::Closed { area } => {
                    let primitive_id = primitive.primitive_id.clone();
                    candidates.push(SuggestionCandidate {
                        sketch_id: sketch.sketch_id.clone(),
                        primitive_id: primitive_id.clone(),
                        area,
                        suggestion: SketchFeatureSuggestion {
                            suggestion_id: format!("{}:{}:extrude", sketch.sketch_id, primitive_id),
                            sketch_id: sketch.sketch_id.clone(),
                            primitive_id: Some(primitive_id.clone()),
                            part_id: safe_part_id(&sketch.sketch_id, &primitive_id),
                            operation: SketchDraftOperationKind::Extrude,
                            amount: DEFAULT_EXTRUDE_AMOUNT,
                            symmetric: false,
                            confidence: 0.82,
                            reason: "closed profile can generate an extruded solid.".to_string(),
                            warnings: Vec::new(),
                        },
                    });
                }
                ProfileMeasure::Warning(message) => warnings.push(format!(
                    "sketch '{}' primitive '{}' {}",
                    sketch.sketch_id, primitive.primitive_id, message
                )),
            }
        }
    }

    candidates.sort_by(|left, right| {
        left.sketch_id
            .cmp(&right.sketch_id)
            .then_with(|| right.area.total_cmp(&left.area))
            .then_with(|| left.primitive_id.cmp(&right.primitive_id))
    });

    let mut suggestions: Vec<SketchFeatureSuggestion> = candidates
        .into_iter()
        .map(|candidate| candidate.suggestion)
        .collect();
    if let Some(limit) = request.limit {
        suggestions.truncate(limit);
    }

    SketchSuggestionResponse {
        suggestions,
        warnings,
    }
}

pub fn sketch_suggestion_to_draft_request(
    sketch: &SketchDefinition,
    suggestion: &SketchFeatureSuggestion,
) -> AppResult<SketchDraftRequest> {
    if sketch.sketch_id != suggestion.sketch_id {
        return Err(AppError::validation(format!(
            "suggestion '{}' belongs to sketch '{}', not '{}'.",
            suggestion.suggestion_id, suggestion.sketch_id, sketch.sketch_id
        )));
    }

    let primitives = match &suggestion.primitive_id {
        Some(primitive_id) => vec![sketch
            .primitives
            .iter()
            .find(|primitive| primitive.primitive_id == *primitive_id)
            .cloned()
            .ok_or_else(|| {
                AppError::validation(format!(
                    "suggestion '{}' references missing primitive '{}'.",
                    suggestion.suggestion_id, primitive_id
                ))
            })?],
        None => sketch.primitives.clone(),
    };

    Ok(SketchDraftRequest {
        part_id: suggestion.part_id.clone(),
        sketch: SketchDefinition {
            sketch_id: sketch.sketch_id.clone(),
            view: sketch.view.clone(),
            plane: sketch.plane.clone(),
            primitives,
            constraints: sketch.constraints.clone(),
        },
        operation: suggestion.operation.clone(),
        amount: suggestion.amount,
        symmetric: suggestion.symmetric,
    })
}

fn sketch_expr(sketch: &SketchDefinition) -> AppResult<String> {
    let mut primitives = Vec::with_capacity(sketch.primitives.len());
    for primitive in &sketch.primitives {
        primitives.push(primitive_expr(primitive)?);
    }
    if primitives.len() == 1 {
        Ok(primitives.remove(0))
    } else {
        Ok(format!("(union {})", primitives.join(" ")))
    }
}

fn compact_sketch_for_source(
    sketch: &SketchDefinition,
    warnings: &mut Vec<String>,
) -> SketchDefinition {
    let primitives = sketch
        .primitives
        .iter()
        .map(|primitive| compact_primitive_for_source(primitive, warnings))
        .collect::<Vec<_>>();
    SketchDefinition {
        sketch_id: sketch.sketch_id.clone(),
        view: sketch.view.clone(),
        plane: sketch.plane.clone(),
        primitives,
        constraints: sketch.constraints.clone(),
    }
}

fn compact_primitive_for_source(
    primitive: &SketchPrimitive,
    warnings: &mut Vec<String>,
) -> SketchPrimitive {
    if !matches!(
        primitive.kind,
        SketchPrimitiveKind::Polyline | SketchPrimitiveKind::Spline
    ) {
        return primitive.clone();
    }
    let compact_points = compact_points_for_source(&primitive.points, primitive.closed);
    if compact_points.len() < primitive.points.len() {
        warnings.push(format!(
            "simplified sketch primitive '{}' from {} to {} points before source generation.",
            primitive.primitive_id,
            primitive.points.len(),
            compact_points.len()
        ));
    }
    SketchPrimitive {
        primitive_id: primitive.primitive_id.clone(),
        kind: primitive.kind.clone(),
        points: compact_points,
        closed: primitive.closed,
        radius: primitive.radius,
    }
}

fn compact_points_for_source(points: &[[f64; 2]], closed: bool) -> Vec<[f64; 2]> {
    let mut cleaned = Vec::with_capacity(points.len());
    for point in points {
        if !point[0].is_finite() || !point[1].is_finite() {
            continue;
        }
        if cleaned
            .last()
            .is_some_and(|last: &[f64; 2]| points_close(last, point))
        {
            continue;
        }
        cleaned.push(*point);
    }

    if closed && cleaned.len() > 1 && points_close(&cleaned[0], cleaned.last().unwrap()) {
        cleaned.pop();
    }

    let max_points = MAX_SOURCE_PRIMITIVE_POINTS;
    if cleaned.len() > max_points && max_points > 1 {
        let last = cleaned.len() - 1;
        let mut sampled = Vec::with_capacity(max_points);
        for index in 0..max_points {
            let source_index =
                ((index as f64) * (last as f64) / ((max_points - 1) as f64)).round() as usize;
            let point = cleaned[source_index.min(last)];
            if sampled
                .last()
                .is_none_or(|last| !points_close(last, &point))
            {
                sampled.push(point);
            }
        }
        sampled
    } else {
        cleaned
    }
}

fn points_close(left: &[f64; 2], right: &[f64; 2]) -> bool {
    (left[0] - right[0]).abs() < 1e-9 && (left[1] - right[1]).abs() < 1e-9
}

fn front_profile_expr(profiles: &[HullProfile]) -> AppResult<String> {
    if profiles.is_empty() {
        return Err(AppError::validation(
            "front profile expression requires at least one closed profile.",
        ));
    }
    let classified = classify_hull_profiles(profiles);
    let outers = classified
        .iter()
        .filter(|profile| profile.role == HullProfileRole::Outer)
        .collect::<Vec<_>>();
    let holes = classified
        .iter()
        .filter(|profile| profile.role == HullProfileRole::Hole)
        .collect::<Vec<_>>();

    if holes.is_empty() {
        let mut primitives = outers
            .iter()
            .map(|profile| primitive_expr(&profile.profile.primitive))
            .collect::<AppResult<Vec<_>>>()?;
        if primitives.len() == 1 {
            Ok(primitives.remove(0))
        } else {
            Ok(format!("(union {})", primitives.join(" ")))
        }
    } else if outers.len() == 1 {
        let outer_expr = primitive_expr(&outers[0].profile.primitive)?;
        let hole_exprs = holes
            .iter()
            .map(|profile| primitive_expr(&profile.profile.primitive))
            .collect::<AppResult<Vec<_>>>()?;
        let holes_expr = if hole_exprs.len() == 1 {
            hole_exprs[0].clone()
        } else {
            format!("(list {})", hole_exprs.join(" "))
        };
        Ok(format!(
            "(profile :outer {} :holes {})",
            outer_expr, holes_expr
        ))
    } else {
        Err(AppError::validation(
            "front profile holes require exactly one containing outer profile.",
        ))
    }
}

fn primitive_expr(primitive: &SketchPrimitive) -> AppResult<String> {
    match primitive.kind {
        SketchPrimitiveKind::Polyline => {
            if !primitive.closed {
                return Err(AppError::validation(format!(
                    "sketch primitive '{}' must be closed before it can generate a solid draft.",
                    primitive.primitive_id
                )));
            }
            Ok(format!("(polygon {})", point_list(&primitive.points)))
        }
        SketchPrimitiveKind::Spline => {
            if !primitive.closed {
                return Err(AppError::validation(format!(
                    "sketch primitive '{}' must be closed before it can generate a solid draft.",
                    primitive.primitive_id
                )));
            }
            Ok(format!("(bspline {} #t)", point_list(&primitive.points)))
        }
        SketchPrimitiveKind::Circle => {
            let radius = primitive.radius.ok_or_else(|| {
                AppError::validation(format!(
                    "sketch primitive '{}' circle radius is required.",
                    primitive.primitive_id
                ))
            })?;
            let center = primitive.points.first().copied().unwrap_or([0.0, 0.0]);
            let circle = format!("(circle {})", format_number(radius));
            if is_zero(center[0]) && is_zero(center[1]) {
                Ok(circle)
            } else {
                Ok(format!(
                    "(translate {} {} 0 {})",
                    format_number(center[0]),
                    format_number(center[1]),
                    circle
                ))
            }
        }
        _ => Err(AppError::validation(format!(
            "sketch primitive '{}' kind {:?} cannot generate a solid draft yet.",
            primitive.primitive_id, primitive.kind
        ))),
    }
}

fn point_list(points: &[[f64; 2]]) -> String {
    let parts: Vec<String> = points
        .iter()
        .map(|point| format!("({} {})", format_number(point[0]), format_number(point[1])))
        .collect();
    format!("({})", parts.join(" "))
}

fn candidate_cells_expr(cells: &[SketchBrepCandidateCell]) -> Option<String> {
    let mut operands = cells.iter().map(candidate_cell_expr).collect::<Vec<_>>();
    if operands.is_empty() {
        return None;
    }
    if operands.len() == 1 {
        return Some(operands.remove(0));
    }
    Some(format!("(union\n      {})", operands.join("\n      ")))
}

fn candidate_cell_expr(cell: &SketchBrepCandidateCell) -> String {
    let width = cell.max[0] - cell.min[0];
    let depth = cell.max[1] - cell.min[1];
    let height = cell.max[2] - cell.min[2];
    let center_x = (cell.min[0] + cell.max[0]) * 0.5;
    let center_y = (cell.min[1] + cell.max[1]) * 0.5;
    let min_z = cell.min[2];

    format!(
        "(translate {} {} {}\n        (box {} {} {}))",
        format_number(center_x),
        format_number(center_y),
        format_number(min_z),
        format_number(width),
        format_number(depth),
        format_number(height)
    )
}

fn accepted_candidate_solution_expr(
    document: &SketchDocument,
    solution: &SketchBrepCandidateSolution,
    selected_cells: &[SketchBrepCandidateCell],
) -> AppResult<(String, Vec<String>)> {
    match solution.source_strategy {
        SketchBrepCandidateSourceStrategy::CellUnion => {
            let Some(source_expr) = candidate_cells_expr(selected_cells) else {
                return Err(AppError::validation(format!(
                    "Accepted BRep candidate solution '{}' produced no solid cells.",
                    solution.solution_id
                )));
            };
            Ok((source_expr, Vec::new()))
        }
        SketchBrepCandidateSourceStrategy::FrontProfilePrism => Ok((
            front_profile_prism_expr(document)?,
            vec!["accepted exact front-profile prism from rectangular depth views".to_string()],
        )),
    }
}

fn front_profile_prism_expr(document: &SketchDocument) -> AppResult<String> {
    let profiles = resolve_preview_hull_profiles(&SketchPreviewHullRequest {
        part_id: "accepted-front-profile-prism".to_string(),
        document: document.clone(),
        fallback_depth: 1.0,
    })?;
    let top = profiles.top.as_ref().ok_or_else(|| {
        AppError::validation("Exact front-profile prism requires a closed Top depth profile.")
    })?;
    let side = profiles.side.as_ref().ok_or_else(|| {
        AppError::validation("Exact front-profile prism requires a closed Side depth profile.")
    })?;
    if !profile_is_axis_aligned_rectangle(&top.primitive)
        || !profile_is_axis_aligned_rectangle(&side.primitive)
    {
        return Err(AppError::validation(
            "Exact front-profile prism requires rectangular Top and Side depth profiles.",
        ));
    }
    let front_expr = front_profile_expr(&profiles.front_profiles)?;
    Ok(format!(
        "(translate 0 0 {}\n        (extrude\n          {}\n          {}))",
        format_number(profiles.depth_min),
        front_expr,
        format_number(profiles.depth)
    ))
}

fn ecky_symbol(value: &str, label: &str) -> AppResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || !trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
    {
        return Err(AppError::validation(format!(
            "sketch draft {} '{}' is not a safe Ecky symbol.",
            label, value
        )));
    }
    Ok(trimmed.to_string())
}

fn format_number(value: f64) -> String {
    if is_zero(value) {
        return "0".to_string();
    }
    if (value.fract()).abs() < 1e-9 {
        return format!("{}", value.round() as i64);
    }
    let mut rendered = format!("{:.6}", value);
    while rendered.contains('.') && rendered.ends_with('0') {
        rendered.pop();
    }
    if rendered.ends_with('.') {
        rendered.pop();
    }
    rendered
}

fn is_zero(value: f64) -> bool {
    value.abs() < 1e-9
}

fn sketch_document_source_map_comment(
    part_id: &str,
    sketch: &SketchDefinition,
    warnings: &mut Vec<String>,
) -> AppResult<String> {
    let document = SketchDocument {
        document_id: part_id.to_string(),
        sketches: vec![sketch.clone()],
        active_sketch_id: Some(sketch.sketch_id.clone()),
        units: None,
        metadata: None,
    };
    let json = serde_json::to_string(&document).map_err(|err| {
        AppError::validation(format!(
            "Failed to serialize sketch draft source map: {}",
            err
        ))
    })?;
    if json.len() > MAX_SOURCE_MAP_JSON_BYTES {
        warnings.push(format!(
            "omitted sketch source map because compacted document is {} bytes, above {} byte limit.",
            json.len(),
            MAX_SOURCE_MAP_JSON_BYTES
        ));
        return Ok("; ecky-sketch-document-source-map: omitted-large-sketch\n".to_string());
    }
    Ok(format!(
        "; ecky-sketch-document-base64: {}\n",
        STANDARD.encode(json)
    ))
}

fn sketch_document_source_map_comment_from_document(
    document: &SketchDocument,
) -> AppResult<String> {
    let json = serde_json::to_string(document).map_err(|err| {
        AppError::validation(format!(
            "Failed to serialize sketch preview hull source map: {}",
            err
        ))
    })?;
    Ok(format!(
        "; ecky-sketch-document-base64: {}\n",
        STANDARD.encode(json)
    ))
}

#[derive(Debug, Clone)]
struct HullProfile {
    sketch_id: String,
    primitive: SketchPrimitive,
    bounds: ProfileBounds,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HullProfileRole {
    Outer,
    Hole,
}

#[derive(Debug, Clone, Copy)]
struct ClassifiedHullProfile<'a> {
    profile: &'a HullProfile,
    role: HullProfileRole,
}

#[derive(Debug, Clone)]
struct ProfileBounds {
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
    width: f64,
    height: f64,
}

impl ProfileBounds {
    fn center_x(&self) -> f64 {
        (self.min_x + self.max_x) * 0.5
    }

    fn center_y(&self) -> f64 {
        (self.min_y + self.max_y) * 0.5
    }
}

#[derive(Debug, Clone)]
struct PreviewHullProfiles {
    front: HullProfile,
    front_profiles: Vec<HullProfile>,
    top: Option<HullProfile>,
    side: Option<HullProfile>,
    depth_min: f64,
    depth: f64,
}

impl PreviewHullProfiles {
    fn view_label(&self) -> String {
        let mut views = vec!["front"];
        if self.top.is_some() {
            views.push("top");
        }
        if self.side.is_some() {
            views.push("side");
        }
        views.join("/")
    }
}

fn resolve_preview_hull_profiles(
    request: &SketchPreviewHullRequest,
) -> AppResult<PreviewHullProfiles> {
    let front_profiles = find_closed_profiles(&request.document, SketchView::Front);
    let front = front_profiles
        .iter()
        .max_by(|left, right| {
            primitive_area_abs(&left.primitive).total_cmp(&primitive_area_abs(&right.primitive))
        })
        .cloned()
        .ok_or_else(|| AppError::validation("Preview hull requires a closed Front profile."))?;
    let top = find_closed_profile(&request.document, SketchView::Top);
    let side = find_closed_profile(&request.document, SketchView::Side);

    if top.is_none() && side.is_none() {
        return Err(AppError::validation(
            "Preview hull requires at least one closed Top or Side profile.",
        ));
    }

    if let Some(top_profile) = &top {
        validate_same_dimension(
            "Top view width",
            top_profile.bounds.width,
            "Front view width",
            front.bounds.width,
        )?;
        validate_same_range(
            "Top view x range",
            top_profile.bounds.min_x,
            top_profile.bounds.max_x,
            "Front view x range",
            front.bounds.min_x,
            front.bounds.max_x,
        )?;
    }

    if let Some(side_profile) = &side {
        validate_same_dimension(
            "Side view height",
            side_profile.bounds.height,
            "Front view height",
            front.bounds.height,
        )?;
        validate_same_range(
            "Side view y range",
            side_profile.bounds.min_y,
            side_profile.bounds.max_y,
            "Front view y range",
            front.bounds.min_y,
            front.bounds.max_y,
        )?;
    }

    let (depth_min, depth_max) = match (&top, &side) {
        (Some(top_profile), Some(side_profile)) => {
            validate_same_dimension(
                "Top view depth",
                top_profile.bounds.height,
                "Side view depth",
                side_profile.bounds.width,
            )?;
            validate_same_range(
                "Top view depth range",
                top_profile.bounds.min_y,
                top_profile.bounds.max_y,
                "Side view depth range",
                side_profile.bounds.min_x,
                side_profile.bounds.max_x,
            )?;
            (top_profile.bounds.min_y, top_profile.bounds.max_y)
        }
        (Some(top_profile), None) => (top_profile.bounds.min_y, top_profile.bounds.max_y),
        (None, Some(side_profile)) => (side_profile.bounds.min_x, side_profile.bounds.max_x),
        (None, None) => (0.0, request.fallback_depth),
    };

    let depth = depth_max - depth_min;
    if !depth.is_finite() || depth <= 0.0 {
        return Err(AppError::validation(
            "Preview hull depth must be positive and finite.",
        ));
    }

    Ok(PreviewHullProfiles {
        front,
        front_profiles,
        top,
        side,
        depth_min,
        depth,
    })
}

fn find_closed_profile(document: &SketchDocument, view: SketchView) -> Option<HullProfile> {
    find_closed_profiles(document, view).into_iter().next()
}

fn find_closed_profiles(document: &SketchDocument, view: SketchView) -> Vec<HullProfile> {
    let mut profiles = Vec::new();
    for sketch in &document.sketches {
        if sketch.view != view {
            continue;
        }
        for primitive in &sketch.primitives {
            if !primitive.closed {
                continue;
            }
            let Some(bounds) = primitive_bounds(primitive) else {
                continue;
            };
            profiles.push(HullProfile {
                sketch_id: sketch.sketch_id.clone(),
                primitive: primitive.clone(),
                bounds,
            });
        }
    }
    profiles
}

fn classify_hull_profiles(profiles: &[HullProfile]) -> Vec<ClassifiedHullProfile<'_>> {
    profiles
        .iter()
        .map(|profile| {
            let sample = primitive_representative_point(&profile.primitive)
                .unwrap_or([profile.bounds.center_x(), profile.bounds.center_y()]);
            let area = primitive_area_abs(&profile.primitive);
            let containing_larger_count = profiles
                .iter()
                .filter(|other| {
                    !std::ptr::eq(*other, profile)
                        && primitive_area_abs(&other.primitive) > area
                        && primitive_contains_point(&other.primitive, sample)
                })
                .count();
            ClassifiedHullProfile {
                profile,
                role: if containing_larger_count % 2 == 1 {
                    HullProfileRole::Hole
                } else {
                    HullProfileRole::Outer
                },
            }
        })
        .collect()
}

fn primitive_bounds(primitive: &SketchPrimitive) -> Option<ProfileBounds> {
    match primitive.kind {
        SketchPrimitiveKind::Circle => {
            let radius = primitive.radius?;
            if !radius.is_finite() || radius <= 0.0 {
                return None;
            }
            let center = primitive.points.first().copied().unwrap_or([0.0, 0.0]);
            if !center[0].is_finite() || !center[1].is_finite() {
                return None;
            }
            Some(ProfileBounds {
                min_x: center[0] - radius,
                max_x: center[0] + radius,
                min_y: center[1] - radius,
                max_y: center[1] + radius,
                width: radius * 2.0,
                height: radius * 2.0,
            })
        }
        _ => bounds_from_points(&primitive.points),
    }
}

fn primitive_area_abs(primitive: &SketchPrimitive) -> f64 {
    match primitive.kind {
        SketchPrimitiveKind::Circle => primitive
            .radius
            .filter(|radius| radius.is_finite() && *radius > 0.0)
            .map(|radius| std::f64::consts::PI * radius * radius)
            .unwrap_or(0.0),
        _ => polygon_area(&primitive.points).abs(),
    }
}

fn primitive_representative_point(primitive: &SketchPrimitive) -> Option<[f64; 2]> {
    primitive
        .points
        .iter()
        .copied()
        .find(|point| point[0].is_finite() && point[1].is_finite())
}

fn primitive_contains_point(primitive: &SketchPrimitive, point: [f64; 2]) -> bool {
    match primitive.kind {
        SketchPrimitiveKind::Circle => {
            let Some(radius) = primitive.radius else {
                return false;
            };
            if !radius.is_finite() || radius <= 0.0 {
                return false;
            }
            let center = primitive.points.first().copied().unwrap_or([0.0, 0.0]);
            if !center[0].is_finite() || !center[1].is_finite() {
                return false;
            }
            (point[0] - center[0]).powi(2) + (point[1] - center[1]).powi(2)
                <= radius.powi(2) + DIMENSION_TOLERANCE
        }
        _ => point_in_polygon_points(point, &logical_profile_points(primitive)),
    }
}

fn profile_is_axis_aligned_rectangle(primitive: &SketchPrimitive) -> bool {
    let points = logical_profile_points(primitive);
    if points.len() != 4 {
        return false;
    }
    let xs = sorted_unique_values(points.iter().map(|point| point[0]).collect());
    let ys = sorted_unique_values(points.iter().map(|point| point[1]).collect());
    if xs.len() != 2 || ys.len() != 2 {
        return false;
    }
    let corners = points
        .iter()
        .map(|point| Point2Key::new(point[0], point[1]))
        .collect::<HashSet<_>>();
    xs.iter()
        .all(|x| ys.iter().all(|y| corners.contains(&Point2Key::new(*x, *y))))
}

fn bounds_from_points(points: &[[f64; 2]]) -> Option<ProfileBounds> {
    let first = points.first()?;
    if !first[0].is_finite() || !first[1].is_finite() {
        return None;
    }
    let (mut min_x, mut max_x) = (first[0], first[0]);
    let (mut min_y, mut max_y) = (first[1], first[1]);
    for point in points.iter().skip(1) {
        if !point[0].is_finite() || !point[1].is_finite() {
            return None;
        }
        min_x = min_x.min(point[0]);
        max_x = max_x.max(point[0]);
        min_y = min_y.min(point[1]);
        max_y = max_y.max(point[1]);
    }
    Some(ProfileBounds {
        min_x,
        max_x,
        min_y,
        max_y,
        width: max_x - min_x,
        height: max_y - min_y,
    })
}

fn validate_same_dimension(
    left_label: &str,
    left_value: f64,
    right_label: &str,
    right_value: f64,
) -> AppResult<()> {
    if same_dimension(left_value, right_value) {
        return Ok(());
    }

    Err(AppError::validation(format!(
        "{} {}mm must match {} {}mm.",
        left_label,
        format_number(left_value),
        right_label,
        format_number(right_value)
    )))
}

fn validate_same_range(
    left_label: &str,
    left_min: f64,
    left_max: f64,
    right_label: &str,
    right_min: f64,
    right_max: f64,
) -> AppResult<()> {
    if same_dimension(left_min, right_min) && same_dimension(left_max, right_max) {
        return Ok(());
    }

    Err(AppError::validation(format!(
        "{} {}..{}mm must match {} {}..{}mm.",
        left_label,
        format_number(left_min),
        format_number(left_max),
        right_label,
        format_number(right_min),
        format_number(right_max)
    )))
}

fn same_dimension(left: f64, right: f64) -> bool {
    (left - right).abs() <= DIMENSION_TOLERANCE
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Point2Key {
    u: i64,
    v: i64,
}

impl Point2Key {
    fn new(u: f64, v: f64) -> Self {
        Self {
            u: quantize(u),
            v: quantize(v),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Point3Key {
    x: i64,
    y: i64,
    z: i64,
}

impl Point3Key {
    fn new(x: f64, y: f64, z: f64) -> Self {
        Self {
            x: quantize(x),
            y: quantize(y),
            z: quantize(z),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Edge2Key {
    a: Point2Key,
    b: Point2Key,
}

impl Edge2Key {
    fn new(a: Point2Key, b: Point2Key) -> Option<Self> {
        if a == b {
            return None;
        }
        if a <= b {
            Some(Self { a, b })
        } else {
            Some(Self { a: b, b: a })
        }
    }
}

#[derive(Debug, Clone)]
struct CandidatePoint {
    key: Point2Key,
    x: f64,
    y: f64,
}

#[derive(Debug, Clone)]
struct CandidateView {
    view: SketchView,
    sketch_id: String,
    primitive_id: String,
    points: Vec<CandidatePoint>,
    loops: Vec<Vec<CandidatePoint>>,
    edges: HashSet<Edge2Key>,
}

impl CandidateView {
    fn from_profile(profile: HullProfile, view: SketchView) -> Self {
        Self::from_profiles(vec![profile], view)
    }

    fn from_profiles(profiles: Vec<HullProfile>, view: SketchView) -> Self {
        let sketch_id = profiles
            .first()
            .map(|profile| profile.sketch_id.clone())
            .unwrap_or_default();
        let primitive_id = profiles
            .first()
            .map(|profile| profile.primitive.primitive_id.clone())
            .unwrap_or_default();
        let loops = profiles
            .iter()
            .map(|profile| {
                logical_profile_points(&profile.primitive)
                    .into_iter()
                    .map(|point| CandidatePoint {
                        key: Point2Key::new(point[0], point[1]),
                        x: point[0],
                        y: point[1],
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|points| points.len() >= 3)
            .collect::<Vec<_>>();
        let points = loops
            .iter()
            .flat_map(|loop_points| loop_points.iter().cloned())
            .collect::<Vec<_>>();
        let mut edges = HashSet::new();
        for loop_points in &loops {
            for index in 0..loop_points.len() {
                let next = (index + 1) % loop_points.len();
                if let Some(edge) = Edge2Key::new(loop_points[index].key, loop_points[next].key) {
                    edges.insert(edge);
                }
            }
        }
        Self {
            view,
            sketch_id,
            primitive_id,
            points,
            loops,
            edges,
        }
    }

    fn has_edge(&self, a: Point2Key, b: Point2Key) -> bool {
        Edge2Key::new(a, b)
            .map(|edge| self.edges.contains(&edge))
            .unwrap_or(false)
    }

    fn contains_point(&self, point: [f64; 2]) -> bool {
        self.loops
            .iter()
            .filter(|loop_points| point_in_polygon(point, loop_points))
            .count()
            % 2
            == 1
    }

    fn is_axis_aligned_rectangle(&self) -> bool {
        if self.points.len() != 4 {
            return false;
        }
        let xs = sorted_unique_values(self.points.iter().map(|point| point.x).collect());
        let ys = sorted_unique_values(self.points.iter().map(|point| point.y).collect());
        if xs.len() != 2 || ys.len() != 2 {
            return false;
        }
        let corners = self
            .points
            .iter()
            .map(|point| point.key)
            .collect::<HashSet<_>>();
        xs.iter()
            .all(|x| ys.iter().all(|y| corners.contains(&Point2Key::new(*x, *y))))
    }
}

#[derive(Debug, Clone)]
struct CandidateViews {
    front: CandidateView,
    top: Option<CandidateView>,
    side: Option<CandidateView>,
}

impl CandidateViews {
    fn new(front: Vec<HullProfile>, top: Option<HullProfile>, side: Option<HullProfile>) -> Self {
        Self {
            front: CandidateView::from_profiles(front, SketchView::Front),
            top: top.map(|profile| CandidateView::from_profile(profile, SketchView::Top)),
            side: side.map(|profile| CandidateView::from_profile(profile, SketchView::Side)),
        }
    }

    fn all(&self) -> Vec<&CandidateView> {
        let mut views = vec![&self.front];
        if let Some(top) = &self.top {
            views.push(top);
        }
        if let Some(side) = &self.side {
            views.push(side);
        }
        views
    }
}

#[derive(Debug, Clone)]
struct CandidateVertexInternal {
    output: SketchBrepCandidateVertex,
    front: Point2Key,
    top: Option<Point2Key>,
    side: Option<Point2Key>,
}

fn build_candidate_vertices(views: &CandidateViews) -> Vec<SketchBrepCandidateVertex> {
    build_candidate_vertex_internals(views)
        .into_iter()
        .map(|vertex| vertex.output)
        .collect()
}

fn build_candidate_vertex_internals(views: &CandidateViews) -> Vec<CandidateVertexInternal> {
    let mut by_point: HashMap<Point3Key, CandidateVertexInternal> = HashMap::new();

    match (&views.top, &views.side) {
        (Some(top), Some(side)) => {
            for front in &views.front.points {
                for top_point in &top.points {
                    if front.key.u != top_point.key.u {
                        continue;
                    }
                    for side_point in &side.points {
                        if front.key.v == side_point.key.v && top_point.key.v == side_point.key.u {
                            insert_candidate_vertex(
                                &mut by_point,
                                front.x,
                                front.y,
                                top_point.y,
                                front.key,
                                Some(top_point.key),
                                Some(side_point.key),
                            );
                        }
                    }
                }
            }
        }
        (Some(top), None) => {
            for front in &views.front.points {
                for top_point in &top.points {
                    if front.key.u == top_point.key.u {
                        insert_candidate_vertex(
                            &mut by_point,
                            front.x,
                            front.y,
                            top_point.y,
                            front.key,
                            Some(top_point.key),
                            None,
                        );
                    }
                }
            }
        }
        (None, Some(side)) => {
            for front in &views.front.points {
                for side_point in &side.points {
                    if front.key.v == side_point.key.v {
                        insert_candidate_vertex(
                            &mut by_point,
                            front.x,
                            front.y,
                            side_point.x,
                            front.key,
                            None,
                            Some(side_point.key),
                        );
                    }
                }
            }
        }
        (None, None) => {}
    }

    let mut vertices = by_point.into_values().collect::<Vec<_>>();
    vertices.sort_by_key(|vertex| {
        Point3Key::new(
            vertex.output.point[0],
            vertex.output.point[1],
            vertex.output.point[2],
        )
    });
    for (index, vertex) in vertices.iter_mut().enumerate() {
        vertex.output.vertex_id = format!("v{}", index);
    }
    vertices
}

fn insert_candidate_vertex(
    by_point: &mut HashMap<Point3Key, CandidateVertexInternal>,
    x: f64,
    y: f64,
    z: f64,
    front: Point2Key,
    top: Option<Point2Key>,
    side: Option<Point2Key>,
) {
    let key = Point3Key::new(x, y, z);
    by_point.entry(key).or_insert_with(|| {
        let mut evidence_views = vec![SketchView::Front];
        if top.is_some() {
            evidence_views.push(SketchView::Top);
        }
        if side.is_some() {
            evidence_views.push(SketchView::Side);
        }
        CandidateVertexInternal {
            output: SketchBrepCandidateVertex {
                vertex_id: String::new(),
                point: [
                    format_constraint_value(x),
                    format_constraint_value(y),
                    format_constraint_value(z),
                ],
                evidence_views,
            },
            front,
            top,
            side,
        }
    });
}

fn build_candidate_edges(
    views: &CandidateViews,
    vertices: &[SketchBrepCandidateVertex],
) -> Vec<SketchBrepCandidateEdge> {
    let internals = build_candidate_vertex_internals(views);
    let id_by_point = vertices
        .iter()
        .map(|vertex| {
            (
                Point3Key::new(vertex.point[0], vertex.point[1], vertex.point[2]),
                vertex.vertex_id.clone(),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut edges = Vec::new();

    for left_index in 0..internals.len() {
        for right_index in (left_index + 1)..internals.len() {
            let left = &internals[left_index];
            let right = &internals[right_index];
            let mut support_views = Vec::new();

            if views.front.has_edge(left.front, right.front) {
                support_views.push(SketchView::Front);
            }
            if let (Some(top), Some(left_top), Some(right_top)) = (&views.top, left.top, right.top)
            {
                if top.has_edge(left_top, right_top) {
                    support_views.push(SketchView::Top);
                }
            }
            if let (Some(side), Some(left_side), Some(right_side)) =
                (&views.side, left.side, right.side)
            {
                if side.has_edge(left_side, right_side) {
                    support_views.push(SketchView::Side);
                }
            }

            if support_views.len() < 2 {
                continue;
            }
            if changed_axis_count(left.output.point, right.output.point) != 1
                && support_views.len() < 3
            {
                continue;
            }

            let left_key = Point3Key::new(
                left.output.point[0],
                left.output.point[1],
                left.output.point[2],
            );
            let right_key = Point3Key::new(
                right.output.point[0],
                right.output.point[1],
                right.output.point[2],
            );
            let Some(a) = id_by_point.get(&left_key).cloned() else {
                continue;
            };
            let Some(b) = id_by_point.get(&right_key).cloned() else {
                continue;
            };
            edges.push(SketchBrepCandidateEdge {
                edge_id: format!("e{}", edges.len()),
                a,
                b,
                support_views,
            });
        }
    }

    edges
}

fn build_candidate_search(views: &CandidateViews) -> SketchBrepCandidateSearch {
    let xs = candidate_axis_values_x(views);
    let ys = candidate_axis_values_y(views);
    let zs = candidate_axis_values_z(views);

    if xs.len() < 2 || ys.len() < 2 || zs.len() < 2 {
        return SketchBrepCandidateSearch {
            cells: Vec::new(),
            rejected_cell_count: 0,
            solutions: Vec::new(),
            evidence: vec!["searched 0/0 candidate cells".to_string()],
        };
    }

    let total_cell_count = (xs.len() - 1) * (ys.len() - 1) * (zs.len() - 1);
    let mut cells = Vec::new();

    for xi in 0..(xs.len() - 1) {
        for yi in 0..(ys.len() - 1) {
            for zi in 0..(zs.len() - 1) {
                let min = [xs[xi], ys[yi], zs[zi]];
                let max = [xs[xi + 1], ys[yi + 1], zs[zi + 1]];
                if !positive_cell(min, max) {
                    continue;
                }

                let center = [
                    (min[0] + max[0]) * 0.5,
                    (min[1] + max[1]) * 0.5,
                    (min[2] + max[2]) * 0.5,
                ];
                let mut support_views = Vec::new();
                if !point_inside_candidate_view(&views.front, center[0], center[1]) {
                    continue;
                }
                support_views.push(SketchView::Front);

                if let Some(top) = &views.top {
                    if !point_inside_candidate_view(top, center[0], center[2]) {
                        continue;
                    }
                    support_views.push(SketchView::Top);
                }
                if let Some(side) = &views.side {
                    if !point_inside_candidate_view(side, center[2], center[1]) {
                        continue;
                    }
                    support_views.push(SketchView::Side);
                }

                cells.push(SketchBrepCandidateCell {
                    cell_id: format!("cell{}", cells.len()),
                    min: min.map(format_constraint_value),
                    max: max.map(format_constraint_value),
                    support_views,
                });
            }
        }
    }

    let rejected_cell_count = total_cell_count.saturating_sub(cells.len());
    let mut evidence = vec![format!(
        "searched {}/{} candidate cells",
        total_cell_count, total_cell_count
    )];
    evidence.push(format!(
        "selected {} silhouette-consistent {}",
        cells.len(),
        if cells.len() == 1 { "cell" } else { "cells" }
    ));

    let solutions = if cells.is_empty() {
        Vec::new()
    } else {
        let source_strategy = if supports_front_profile_prism_solution(views, cells.len()) {
            SketchBrepCandidateSourceStrategy::FrontProfilePrism
        } else {
            SketchBrepCandidateSourceStrategy::CellUnion
        };
        if source_strategy == SketchBrepCandidateSourceStrategy::FrontProfilePrism {
            evidence.push(
                "front-profile prism strategy available from rectangular depth views".to_string(),
            );
        }
        vec![SketchBrepCandidateSolution {
            solution_id: "solution0".to_string(),
            cell_ids: cells.iter().map(|cell| cell.cell_id.clone()).collect(),
            score: format_constraint_value(cells.len() as f64 / total_cell_count as f64),
            source_strategy,
            evidence: evidence.clone(),
        }]
    };

    SketchBrepCandidateSearch {
        cells,
        rejected_cell_count,
        solutions,
        evidence,
    }
}

fn supports_front_profile_prism_solution(views: &CandidateViews, cell_count: usize) -> bool {
    if cell_count <= 1 {
        return false;
    }
    match (&views.top, &views.side) {
        (Some(top), Some(side)) => {
            top.is_axis_aligned_rectangle() && side.is_axis_aligned_rectangle()
        }
        _ => false,
    }
}

fn has_front_profile_prism_solution(search: &SketchBrepCandidateSearch) -> bool {
    search.solutions.iter().any(|solution| {
        solution.source_strategy == SketchBrepCandidateSourceStrategy::FrontProfilePrism
    })
}

fn candidate_axis_values_x(views: &CandidateViews) -> Vec<f64> {
    let mut values = views
        .front
        .points
        .iter()
        .map(|point| point.x)
        .collect::<Vec<_>>();
    if let Some(top) = &views.top {
        values.extend(top.points.iter().map(|point| point.x));
    }
    sorted_unique_values(values)
}

fn candidate_axis_values_y(views: &CandidateViews) -> Vec<f64> {
    let mut values = views
        .front
        .points
        .iter()
        .map(|point| point.y)
        .collect::<Vec<_>>();
    if let Some(side) = &views.side {
        values.extend(side.points.iter().map(|point| point.y));
    }
    sorted_unique_values(values)
}

fn candidate_axis_values_z(views: &CandidateViews) -> Vec<f64> {
    let mut values = Vec::new();
    if let Some(top) = &views.top {
        values.extend(top.points.iter().map(|point| point.y));
    }
    if let Some(side) = &views.side {
        values.extend(side.points.iter().map(|point| point.x));
    }
    sorted_unique_values(values)
}

fn sorted_unique_values(mut values: Vec<f64>) -> Vec<f64> {
    values.retain(|value| value.is_finite());
    values.sort_by(|left, right| left.total_cmp(right));
    values.dedup_by(|left, right| same_dimension(*left, *right));
    values
}

fn positive_cell(min: [f64; 3], max: [f64; 3]) -> bool {
    max[0] > min[0] && max[1] > min[1] && max[2] > min[2]
}

fn point_inside_candidate_view(view: &CandidateView, x: f64, y: f64) -> bool {
    view.contains_point([x, y])
}

fn point_in_polygon(point: [f64; 2], points: &[CandidatePoint]) -> bool {
    let points = points
        .iter()
        .map(|point| [point.x, point.y])
        .collect::<Vec<_>>();
    point_in_polygon_points(point, &points)
}

fn point_in_polygon_points(point: [f64; 2], points: &[[f64; 2]]) -> bool {
    if points.len() < 3 {
        return false;
    }

    let mut inside = false;
    let mut previous = points.len() - 1;
    for current in 0..points.len() {
        let a = points[current];
        let b = points[previous];
        if point_on_segment(point, a, b) {
            return true;
        }
        let crosses = (a[1] > point[1]) != (b[1] > point[1]);
        if crosses {
            let x_intersection = (b[0] - a[0]) * (point[1] - a[1]) / (b[1] - a[1]) + a[0];
            if point[0] < x_intersection {
                inside = !inside;
            }
        }
        previous = current;
    }
    inside
}

fn point_on_segment(point: [f64; 2], a: [f64; 2], b: [f64; 2]) -> bool {
    let cross = (point[1] - a[1]) * (b[0] - a[0]) - (point[0] - a[0]) * (b[1] - a[1]);
    if cross.abs() > DIMENSION_TOLERANCE {
        return false;
    }
    let dot = (point[0] - a[0]) * (b[0] - a[0]) + (point[1] - a[1]) * (b[1] - a[1]);
    if dot < -DIMENSION_TOLERANCE {
        return false;
    }
    let squared_len = (b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2);
    dot <= squared_len + DIMENSION_TOLERANCE
}

fn validate_candidate_reprojection(
    views: &CandidateViews,
    edges: &[SketchBrepCandidateEdge],
) -> SketchBrepProjectionValidation {
    let internals = build_candidate_vertex_internals(views);
    let vertices_by_id = internals
        .iter()
        .map(|vertex| (vertex.output.vertex_id.clone(), vertex.clone()))
        .collect::<HashMap<_, _>>();
    let mut covered_by_view: HashMap<String, HashSet<Edge2Key>> = HashMap::new();

    for edge in edges {
        let Some(left) = vertices_by_id.get(&edge.a) else {
            continue;
        };
        let Some(right) = vertices_by_id.get(&edge.b) else {
            continue;
        };
        for support in &edge.support_views {
            if let Some(projected) = projected_edge_for_view(support, left, right) {
                covered_by_view
                    .entry(view_key(support).to_string())
                    .or_default()
                    .insert(projected);
            }
        }
    }

    let mut evidence = Vec::new();
    let mut issues = Vec::new();
    for view in views.all() {
        let key = view_key(&view.view);
        let covered = covered_by_view.get(key).map(|set| set.len()).unwrap_or(0);
        let total = view.edges.len();
        evidence.push(format!("{key} {covered}/{total} edges covered"));
        if covered < total {
            issues.push(SketchValidationIssue {
                sketch_id: view.sketch_id.clone(),
                primitive_id: Some(view.primitive_id.clone()),
                severity: SketchValidationSeverity::Error,
                message: format!(
                    "{} projection replay covers {}/{} source edges.",
                    view_label(&view.view),
                    covered,
                    total
                ),
            });
        }
    }

    if internals.is_empty() {
        issues.push(SketchValidationIssue {
            sketch_id: "candidate-graph".to_string(),
            primitive_id: None,
            severity: SketchValidationSeverity::Error,
            message: "BRep candidate graph has no vertices.".to_string(),
        });
    }
    if edges.is_empty() {
        issues.push(SketchValidationIssue {
            sketch_id: "candidate-graph".to_string(),
            primitive_id: None,
            severity: SketchValidationSeverity::Error,
            message: "BRep candidate graph has no edges with two-view support.".to_string(),
        });
    }

    SketchBrepProjectionValidation {
        passed: issues.is_empty(),
        issues,
        evidence,
    }
}

fn validate_front_profile_prism_solution(views: &CandidateViews) -> SketchBrepProjectionValidation {
    let mut evidence = vec![format!(
        "front {}/{} edges preserved by exact front-profile prism",
        views.front.edges.len(),
        views.front.edges.len()
    )];
    if let Some(top) = &views.top {
        evidence.push(format!(
            "top {}/{} rectangular depth edges covered by front-profile prism",
            top.edges.len(),
            top.edges.len()
        ));
    }
    if let Some(side) = &views.side {
        evidence.push(format!(
            "side {}/{} rectangular depth edges covered by front-profile prism",
            side.edges.len(),
            side.edges.len()
        ));
    }
    SketchBrepProjectionValidation {
        passed: true,
        issues: Vec::new(),
        evidence,
    }
}

fn projected_edge_for_view(
    view: &SketchView,
    left: &CandidateVertexInternal,
    right: &CandidateVertexInternal,
) -> Option<Edge2Key> {
    match view {
        SketchView::Front => Edge2Key::new(left.front, right.front),
        SketchView::Top => Edge2Key::new(left.top?, right.top?),
        SketchView::Side => Edge2Key::new(left.side?, right.side?),
        SketchView::Custom => None,
    }
}

fn changed_axis_count(left: [f64; 3], right: [f64; 3]) -> usize {
    [
        !same_dimension(left[0], right[0]),
        !same_dimension(left[1], right[1]),
        !same_dimension(left[2], right[2]),
    ]
    .into_iter()
    .filter(|changed| *changed)
    .count()
}

fn logical_profile_points(primitive: &SketchPrimitive) -> Vec<[f64; 2]> {
    let mut points = primitive.points.clone();
    if points.len() > 1 {
        let first = points[0];
        let last = points[points.len() - 1];
        if same_dimension(first[0], last[0]) && same_dimension(first[1], last[1]) {
            points.pop();
        }
    }
    points
}

fn quantize(value: f64) -> i64 {
    (value * 1_000_000.0).round() as i64
}

fn format_constraint_value(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}

fn view_key(view: &SketchView) -> &'static str {
    match view {
        SketchView::Front => "front",
        SketchView::Top => "top",
        SketchView::Side => "side",
        SketchView::Custom => "custom",
    }
}

fn view_label(view: &SketchView) -> &'static str {
    match view {
        SketchView::Front => "Front",
        SketchView::Top => "Top",
        SketchView::Side => "Side",
        SketchView::Custom => "Custom",
    }
}

struct SuggestionCandidate {
    sketch_id: String,
    primitive_id: String,
    area: f64,
    suggestion: SketchFeatureSuggestion,
}

enum ProfileMeasure {
    Closed { area: f64 },
    Warning(String),
}

fn profile_measure(primitive: &SketchPrimitive) -> ProfileMeasure {
    match primitive.kind {
        SketchPrimitiveKind::Polyline | SketchPrimitiveKind::Spline => {
            if !primitive.closed {
                return ProfileMeasure::Warning(
                    "is open; close it before creating a solid draft.".to_string(),
                );
            }
            if primitive.points.len() < 3 {
                return ProfileMeasure::Warning(
                    "has fewer than 3 points; add a closed profile.".to_string(),
                );
            }
            if !points_have_finite_coordinates(&primitive.points) {
                return ProfileMeasure::Warning(
                    "has invalid coordinates; fix the profile before drafting.".to_string(),
                );
            };
            ProfileMeasure::Closed {
                area: polygon_area(&primitive.points).abs(),
            }
        }
        SketchPrimitiveKind::Circle => {
            let Some(radius) = primitive.radius else {
                return ProfileMeasure::Warning(
                    "is a circle without radius; add radius before drafting.".to_string(),
                );
            };
            if !radius.is_finite() || radius <= 0.0 {
                return ProfileMeasure::Warning(
                    "is a circle with invalid radius; use a positive finite radius.".to_string(),
                );
            }
            ProfileMeasure::Closed {
                area: std::f64::consts::PI * radius * radius,
            }
        }
        _ => ProfileMeasure::Warning(format!(
            "kind {:?} cannot generate a solid draft yet.",
            primitive.kind
        )),
    }
}

fn polygon_area(points: &[[f64; 2]]) -> f64 {
    let mut area = 0.0;
    for index in 0..points.len() {
        let next = (index + 1) % points.len();
        area += points[index][0] * points[next][1] - points[next][0] * points[index][1];
    }
    area / 2.0
}

fn points_have_finite_coordinates(points: &[[f64; 2]]) -> bool {
    let Some(first) = points.first() else {
        return false;
    };
    if !first[0].is_finite() || !first[1].is_finite() {
        return false;
    }
    for point in points.iter().skip(1) {
        if !point[0].is_finite() || !point[1].is_finite() {
            return false;
        }
    }
    true
}

fn safe_part_id(sketch_id: &str, primitive_id: &str) -> String {
    let raw = format!("{}_{}", sketch_id, primitive_id);
    let mut rendered = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-') {
            rendered.push(ch);
        } else {
            rendered.push('_');
        }
    }
    if rendered.is_empty() {
        "sketch_feature".to_string()
    } else {
        rendered
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

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

    fn polyline(id: &str, points: Vec<[f64; 2]>) -> SketchPrimitive {
        SketchPrimitive {
            primitive_id: id.to_string(),
            kind: SketchPrimitiveKind::Polyline,
            points,
            radius: None,
            closed: true,
        }
    }

    fn sketch(sketch_id: &str, view: SketchView, primitive: SketchPrimitive) -> SketchDefinition {
        SketchDefinition {
            sketch_id: sketch_id.to_string(),
            view,
            plane: None,
            primitives: vec![primitive],
            constraints: Vec::new(),
        }
    }

    fn three_view_box_document() -> SketchDocument {
        SketchDocument {
            document_id: "candidate-doc".to_string(),
            sketches: vec![
                sketch(
                    "sketch-front",
                    SketchView::Front,
                    polyline(
                        "front",
                        vec![
                            [10.0, 20.0],
                            [60.0, 20.0],
                            [60.0, 50.0],
                            [10.0, 50.0],
                            [10.0, 20.0],
                        ],
                    ),
                ),
                sketch(
                    "sketch-top",
                    SketchView::Top,
                    polyline(
                        "top",
                        vec![
                            [10.0, 5.0],
                            [60.0, 5.0],
                            [60.0, 27.0],
                            [10.0, 27.0],
                            [10.0, 5.0],
                        ],
                    ),
                ),
                sketch(
                    "sketch-side",
                    SketchView::Side,
                    polyline(
                        "side",
                        vec![
                            [5.0, 20.0],
                            [27.0, 20.0],
                            [27.0, 50.0],
                            [5.0, 50.0],
                            [5.0, 20.0],
                        ],
                    ),
                ),
            ],
            active_sketch_id: Some("sketch-front".to_string()),
            units: Some("mm".to_string()),
            metadata: None,
        }
    }

    fn concave_front_document() -> SketchDocument {
        SketchDocument {
            document_id: "candidate-concave-doc".to_string(),
            sketches: vec![
                sketch(
                    "sketch-front",
                    SketchView::Front,
                    polyline(
                        "front",
                        vec![
                            [0.0, 0.0],
                            [20.0, 0.0],
                            [20.0, 10.0],
                            [10.0, 10.0],
                            [10.0, 20.0],
                            [0.0, 20.0],
                            [0.0, 0.0],
                        ],
                    ),
                ),
                sketch(
                    "sketch-top",
                    SketchView::Top,
                    polyline(
                        "top",
                        vec![
                            [0.0, 0.0],
                            [20.0, 0.0],
                            [20.0, 10.0],
                            [0.0, 10.0],
                            [0.0, 0.0],
                        ],
                    ),
                ),
                sketch(
                    "sketch-side",
                    SketchView::Side,
                    polyline(
                        "side",
                        vec![
                            [0.0, 0.0],
                            [10.0, 0.0],
                            [10.0, 20.0],
                            [0.0, 20.0],
                            [0.0, 0.0],
                        ],
                    ),
                ),
            ],
            active_sketch_id: Some("sketch-front".to_string()),
            units: Some("mm".to_string()),
            metadata: None,
        }
    }

    #[test]
    fn analyze_sketch_brep_candidates_searches_silhouette_cells() {
        let response = analyze_sketch_brep_candidates(SketchBrepCandidateRequest {
            document: three_view_box_document(),
        })
        .expect("candidate search should succeed");

        assert_eq!(response.search.cells.len(), 1);
        assert_eq!(response.search.solutions.len(), 1);
        assert_eq!(response.search.cells[0].min, [10.0, 20.0, 5.0]);
        assert_eq!(response.search.cells[0].max, [60.0, 50.0, 27.0]);
        assert_eq!(response.search.rejected_cell_count, 0);
        assert!(response.search.evidence[0].contains("searched 1/1 candidate cells"));
    }

    #[test]
    fn analyze_sketch_brep_candidates_promotes_concave_prism_strategy() {
        let response = analyze_sketch_brep_candidates(SketchBrepCandidateRequest {
            document: concave_front_document(),
        })
        .expect("concave candidate search should succeed");

        assert!(response.validation.passed);
        assert_eq!(response.search.cells.len(), 3);
        assert_eq!(response.search.rejected_cell_count, 1);
        assert_eq!(response.search.solutions[0].cell_ids.len(), 3);
        assert_eq!(
            response.search.solutions[0].source_strategy,
            SketchBrepCandidateSourceStrategy::FrontProfilePrism
        );
        assert!(response.search.evidence[1].contains("selected 3 silhouette-consistent cells"));
    }

    #[test]
    fn live_accepted_brep_candidate_source_exports_step_when_sdk_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root =
            crate::ecky_cad_host::direct_occt_sdk::bundled_build123d_runtime_root_from_repo(
                repo_root,
            );
        if !runtime_root.exists() {
            return;
        }
        let layout =
            crate::ecky_cad_host::direct_occt_sdk::inspect_build123d_ocp_runtime(&runtime_root);
        if !layout.can_compile_native_shim() {
            return;
        }

        let root = std::env::temp_dir().join(format!(
            "ecky-accepted-brep-direct-{}",
            uuid::Uuid::new_v4()
        ));
        let resolver = TestResolver { root: root.clone() };
        let accepted = generate_accepted_brep_candidate_source(SketchBrepCandidateAcceptRequest {
            part_id: "accepted-body".to_string(),
            document: three_view_box_document(),
            solution_id: "solution0".to_string(),
            tolerance: None,
        })
        .expect("accepted source");
        let program = crate::ecky_scheme::compile_to_core_program(&accepted.draft_source.source)
            .expect("accepted source compiles");

        let (bundle, _manifest) =
            crate::ecky_cad_host::direct_occt_runtime::render_core_program_runtime_bundle(
                &program,
                &accepted.draft_source.source,
                &DesignParams::new(),
                &layout,
                &resolver,
            )
            .expect("direct STEP bundle");
        let step_artifact =
            require_step_export_artifact(&bundle).expect("accepted candidate STEP artifact");

        assert!(Path::new(&bundle.preview_stl_path).is_file());
        assert!(Path::new(&step_artifact.path).is_file());

        let accepted_prism =
            generate_accepted_brep_candidate_source(SketchBrepCandidateAcceptRequest {
                part_id: "accepted-concave".to_string(),
                document: concave_front_document(),
                solution_id: "solution0".to_string(),
                tolerance: None,
            })
            .expect("accepted exact prism source");
        assert_eq!(
            accepted_prism.accepted_solution.source_strategy,
            SketchBrepCandidateSourceStrategy::FrontProfilePrism
        );
        let prism_program =
            crate::ecky_scheme::compile_to_core_program(&accepted_prism.draft_source.source)
                .expect("accepted exact prism source compiles");
        let (prism_bundle, _manifest) =
            crate::ecky_cad_host::direct_occt_runtime::render_core_program_runtime_bundle(
                &prism_program,
                &accepted_prism.draft_source.source,
                &DesignParams::new(),
                &layout,
                &resolver,
            )
            .expect("direct STEP bundle for exact prism");
        let prism_step_artifact = require_step_export_artifact(&prism_bundle)
            .expect("accepted exact prism STEP artifact");
        assert!(Path::new(&prism_bundle.preview_stl_path).is_file());
        assert!(Path::new(&prism_step_artifact.path).is_file());

        let _ = std::fs::remove_dir_all(root);
    }
}
