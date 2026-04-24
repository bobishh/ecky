use crate::models::SketchDocument;
use crate::models::{
    validate_sketch_definition, AppError, AppResult, ArtifactBundle, DesignParams, GeometryBackend,
    MacroDialect, PathResolver, SketchBrepCandidateEdge, SketchBrepCandidateGraph,
    SketchBrepCandidateRequest, SketchBrepCandidateResponse, SketchBrepCandidateVertex,
    SketchBrepProjectionValidation, SketchDefinition, SketchDraftOperationKind, SketchDraftRequest,
    SketchDraftSource, SketchFeatureSuggestion, SketchPreviewHullRequest, SketchPrimitive,
    SketchPrimitiveKind, SketchSuggestionRequest, SketchSuggestionResponse, SketchValidationIssue,
    SketchValidationSeverity, SketchView, SourceLanguage,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json;
use std::collections::{HashMap, HashSet};

const DEFAULT_EXTRUDE_AMOUNT: f64 = 12.0;
const MIN_EXTRUDE_AMOUNT: f64 = 1.0;
const MAX_EXTRUDE_AMOUNT: f64 = 250.0;
const DIMENSION_TOLERANCE: f64 = 1e-6;

pub fn generate_sketch_draft_source(request: SketchDraftRequest) -> AppResult<SketchDraftSource> {
    validate_sketch_definition("draft", &request.sketch)?;
    if !request.amount.is_finite() || request.amount <= 0.0 {
        return Err(AppError::validation(
            "sketch draft amount must be positive and finite.",
        ));
    }
    let part_id = ecky_symbol(&request.part_id, "partId")?;
    let sketch_expr = sketch_expr(&request.sketch)?;
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

    let source_map_comment = sketch_document_source_map_comment(&request)?;

    Ok(SketchDraftSource {
        source_language: SourceLanguage::EckyIrV0,
        geometry_backend: GeometryBackend::EckyRust,
        macro_dialect: MacroDialect::EckyIrV0,
        source: format!(
            "{}(model\n  (part {}\n    {}))\n",
            source_map_comment, part_id, operation_expr
        ),
        warnings: Vec::new(),
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
    let mut operands = Vec::new();

    let front_expr = primitive_expr(&profiles.front.primitive)?;
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

    let hull_expr = if operands.len() == 1 {
        operands.remove(0)
    } else {
        format!("(intersection\n      {})", operands.join("\n      "))
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
            "preview hull from {} silhouettes; not accepted BRep.",
            view_label
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

    let front = find_closed_profile(&request.document, SketchView::Front).ok_or_else(|| {
        AppError::validation("BRep candidate graph requires a closed Front profile.")
    })?;
    let top = find_closed_profile(&request.document, SketchView::Top);
    let side = find_closed_profile(&request.document, SketchView::Side);

    if top.is_none() && side.is_none() {
        return Err(AppError::validation(
            "BRep candidate graph requires a closed Top or Side profile.",
        ));
    }

    let views = CandidateViews::new(front, top, side);
    let vertices = build_candidate_vertices(&views);
    let edges = build_candidate_edges(&views, &vertices);
    let validation = validate_candidate_reprojection(&views, &edges);

    Ok(SketchBrepCandidateResponse {
        graph: SketchBrepCandidateGraph { vertices, edges },
        validation,
    })
}

pub fn suggest_sketch_features(request: SketchSuggestionRequest) -> SketchSuggestionResponse {
    let mut candidates = Vec::new();
    let mut warnings = Vec::new();

    for sketch in &request.document.sketches {
        for primitive in &sketch.primitives {
            match profile_measure(primitive) {
                ProfileMeasure::Closed { area, bbox_min } => {
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
                            amount: default_extrude_amount(bbox_min),
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

fn sketch_document_source_map_comment(request: &SketchDraftRequest) -> AppResult<String> {
    let document = SketchDocument {
        document_id: request.part_id.clone(),
        sketches: vec![request.sketch.clone()],
        active_sketch_id: Some(request.sketch.sketch_id.clone()),
        units: None,
        metadata: None,
    };
    let json = serde_json::to_string(&document).map_err(|err| {
        AppError::validation(format!(
            "Failed to serialize sketch draft source map: {}",
            err
        ))
    })?;
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
    let front = find_closed_profile(&request.document, SketchView::Front)
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
        top,
        side,
        depth_min,
        depth,
    })
}

fn find_closed_profile(document: &SketchDocument, view: SketchView) -> Option<HullProfile> {
    for sketch in &document.sketches {
        if sketch.view != view {
            continue;
        }
        for primitive in &sketch.primitives {
            if !primitive.closed {
                continue;
            }
            let bounds = primitive_bounds(primitive)?;
            return Some(HullProfile {
                sketch_id: sketch.sketch_id.clone(),
                primitive: primitive.clone(),
                bounds,
            });
        }
    }
    None
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
    edges: HashSet<Edge2Key>,
}

impl CandidateView {
    fn from_profile(profile: HullProfile, view: SketchView) -> Self {
        let points = logical_profile_points(&profile.primitive)
            .into_iter()
            .map(|point| CandidatePoint {
                key: Point2Key::new(point[0], point[1]),
                x: point[0],
                y: point[1],
            })
            .collect::<Vec<_>>();
        let mut edges = HashSet::new();
        for index in 0..points.len() {
            let next = (index + 1) % points.len();
            if let Some(edge) = Edge2Key::new(points[index].key, points[next].key) {
                edges.insert(edge);
            }
        }
        Self {
            view,
            sketch_id: profile.sketch_id,
            primitive_id: profile.primitive.primitive_id,
            points,
            edges,
        }
    }

    fn has_edge(&self, a: Point2Key, b: Point2Key) -> bool {
        Edge2Key::new(a, b)
            .map(|edge| self.edges.contains(&edge))
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone)]
struct CandidateViews {
    front: CandidateView,
    top: Option<CandidateView>,
    side: Option<CandidateView>,
}

impl CandidateViews {
    fn new(front: HullProfile, top: Option<HullProfile>, side: Option<HullProfile>) -> Self {
        Self {
            front: CandidateView::from_profile(front, SketchView::Front),
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
    Closed { area: f64, bbox_min: f64 },
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
            let Some(bbox_min) = bbox_min_dimension(&primitive.points) else {
                return ProfileMeasure::Warning(
                    "has invalid coordinates; fix the profile before drafting.".to_string(),
                );
            };
            ProfileMeasure::Closed {
                area: polygon_area(&primitive.points).abs(),
                bbox_min,
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
                bbox_min: radius * 2.0,
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

fn bbox_min_dimension(points: &[[f64; 2]]) -> Option<f64> {
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
    Some((max_x - min_x).min(max_y - min_y))
}

fn default_extrude_amount(bbox_min: f64) -> f64 {
    if !bbox_min.is_finite() || bbox_min <= 0.0 {
        return DEFAULT_EXTRUDE_AMOUNT;
    }
    bbox_min.clamp(MIN_EXTRUDE_AMOUNT, MAX_EXTRUDE_AMOUNT)
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
