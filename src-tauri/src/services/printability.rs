use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

const OVERHANG_NORMAL_Z_THRESHOLD: f32 = -0.70710677;
const BUILD_PLANE_EPSILON_MM: f32 = 0.001;
const ORIENTATION_HEIGHT_TIEBREAKER_WEIGHT: f64 = 0.000_001;
const ORIENTATION_FOOTPRINT_TIEBREAKER_WEIGHT: f64 = 0.000_000_001;
const CHAMFER_OVERHANG_RATIO_THRESHOLD: f64 = 0.25;
const DEFAULT_CHAMFER_MM: f64 = 0.5;
const SPLIT_ASPECT_RATIO_THRESHOLD: f64 = 6.0;
const BRIDGE_SPAN_RISK_THRESHOLD_MM: f64 = 20.0;
const THIN_WALL_RISK_THRESHOLD_MM: f64 = 1.2;
const UNSUPPORTED_ISLAND_TRIANGLE_MAX: usize = 8;
const UNSUPPORTED_ISLAND_MIN_CLEARANCE_MM: f32 = 0.4;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintabilityAnalysis {
    pub triangle_count: u32,
    pub overhang_triangle_count: u32,
    pub overhang_ratio: f64,
    pub bbox: Option<PrintabilityBoundingBox>,
    pub topology: PrintabilityTopologyFacts,
    #[serde(default)]
    pub risk_metrics: PrintabilityRiskMetrics,
    pub orientation_score: OrientationScorePlaceholder,
    #[serde(default)]
    pub transform_suggestions: Vec<PrintabilityTransformSuggestion>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintabilityRiskMetrics {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bridge_span_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thin_wall_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsupported_island_count: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintabilityBoundingBox {
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub size: [f64; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintabilityTopologyFacts {
    pub component_count: Option<u32>,
    pub non_manifold_edge_count: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrientationScorePlaceholder {
    pub current_orientation_score: Option<f64>,
    pub candidates: Vec<OrientationScoreCandidate>,
    pub status: OrientationScoreStatus,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrientationScoreCandidate {
    pub label: String,
    pub score: Option<f64>,
    pub rotation_degrees: [i16; 3],
    pub overhang_ratio: Option<f64>,
    pub bbox_height_mm: Option<f64>,
    pub footprint_area_mm2: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OrientationScoreStatus {
    NotScored,
    Scored,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintabilityTransformSuggestion {
    pub suggestion_id: String,
    pub kind: PrintabilityTransformSuggestionKind,
    pub label: String,
    pub confidence: Option<f64>,
    pub reason: Option<String>,
    pub source_anchor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_anchor: Option<PrintabilityRiskAnchor>,
    pub rotation_degrees: Option<[i16; 3]>,
    pub target_orientation_label: Option<String>,
    pub overhang_ratio_before: Option<f64>,
    pub overhang_ratio_after: Option<f64>,
    pub chamfer_mm: Option<f64>,
    pub split_axis: Option<PrintabilityAxis>,
    pub split_position_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bridge_span_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thin_wall_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsupported_island_count: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PrintabilityTransformSuggestionKind {
    OrientationHint,
    Chamfer,
    Split,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintabilityRiskAnchor {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stable_node_keys: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportlessFdmTransformRecipe {
    pub recipe_id: String,
    pub action_kind: SupportlessFdmRecipeActionKind,
    pub label: String,
    pub rationale: String,
    pub estimated_effect: TransformRecipeEstimatedEffect,
    pub target: Option<String>,
    pub source_anchor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_anchor: Option<PrintabilityRiskAnchor>,
    pub preview_support_status: TransformRecipeSupportStatus,
    pub apply_support_status: TransformRecipeSupportStatus,
    pub rotation_degrees: Option<[i16; 3]>,
    pub chamfer_mm: Option<f64>,
    pub split_axis: Option<PrintabilityAxis>,
    pub split_position_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bridge_span_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thin_wall_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsupported_island_count: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SupportlessFdmRecipeActionKind {
    Reorient,
    Chamfer,
    Split,
    Relief,
    Clearance,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransformRecipeEstimatedEffect {
    pub summary: String,
    pub overhang_ratio_before: Option<f64>,
    pub overhang_ratio_after: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransformRecipeSupportStatus {
    Pending,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PrintabilityAxis {
    X,
    Y,
    Z,
}

#[derive(Debug)]
pub enum PrintabilityError {
    Io(std::io::Error),
    EmptyStl,
    UnreadableStl,
}

impl fmt::Display for PrintabilityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "failed to read STL: {err}"),
            Self::EmptyStl => write!(f, "STL file is empty"),
            Self::UnreadableStl => write!(f, "STL file could not be parsed"),
        }
    }
}

impl std::error::Error for PrintabilityError {}

impl From<std::io::Error> for PrintabilityError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

pub fn analyze_stl_path(path: &Path) -> Result<PrintabilityAnalysis, PrintabilityError> {
    let bytes = fs::read(path)?;
    if bytes.is_empty() {
        return Err(PrintabilityError::EmptyStl);
    }
    let triangles = parse_stl_triangles(&bytes)?;
    Ok(analyze_stl_triangles(&triangles))
}

pub fn analyze_stl_triangles(triangles: &[[[f32; 3]; 3]]) -> PrintabilityAnalysis {
    let indexed_triangles: Vec<IndexedTriangle> = triangles
        .iter()
        .map(|coords| IndexedTriangle {
            vertices: coords.map(StlVertex::new),
            coords: *coords,
        })
        .collect();
    let edge_triangles = stl_edge_triangles(&indexed_triangles);
    let component_metrics = stl_component_metrics(&indexed_triangles, &edge_triangles);
    let overhang_triangle_count = stl_overhang_triangle_count(&indexed_triangles);
    let overhang_ratio = if indexed_triangles.is_empty() {
        0.0
    } else {
        overhang_triangle_count as f64 / indexed_triangles.len() as f64
    };

    let bbox = bounding_box(triangles);
    let risk_metrics = printability_risk_metrics(bbox.as_ref(), &component_metrics);
    let topology = PrintabilityTopologyFacts {
        component_count: Some(usize_metric(component_metrics.len())),
        non_manifold_edge_count: Some(usize_metric(
            edge_triangles
                .values()
                .filter(|triangle_ids| triangle_ids.len() != 2)
                .count(),
        )),
    };
    let orientation_score = orientation_score(&indexed_triangles);
    let transform_suggestions = printability_transform_suggestions(
        overhang_ratio,
        bbox.as_ref(),
        &topology,
        &risk_metrics,
        &orientation_score,
    );

    PrintabilityAnalysis {
        triangle_count: usize_metric(indexed_triangles.len()),
        overhang_triangle_count: usize_metric(overhang_triangle_count),
        overhang_ratio,
        bbox,
        topology,
        risk_metrics,
        orientation_score,
        transform_suggestions,
    }
}

pub fn enrich_transform_suggestions_with_source_anchor(
    analysis: &mut PrintabilityAnalysis,
    source_anchor: Option<String>,
) {
    let Some(source_anchor) = source_anchor
        .map(|anchor| anchor.trim().to_string())
        .filter(|anchor| !anchor.is_empty())
    else {
        return;
    };

    for suggestion in &mut analysis.transform_suggestions {
        if suggestion.source_anchor.is_none() {
            suggestion.source_anchor = Some(source_anchor.clone());
        }
    }
}

pub fn enrich_transform_suggestions_with_risk_anchor(
    analysis: &mut PrintabilityAnalysis,
    risk_anchor: Option<PrintabilityRiskAnchor>,
) {
    let Some(risk_anchor) = risk_anchor else {
        return;
    };
    for suggestion in &mut analysis.transform_suggestions {
        if suggestion.risk_anchor.is_none() {
            suggestion.risk_anchor = Some(risk_anchor.clone());
        }
    }
}

pub fn supportless_fdm_transform_recipes(
    analysis: &PrintabilityAnalysis,
) -> Vec<SupportlessFdmTransformRecipe> {
    analysis
        .transform_suggestions
        .iter()
        .filter_map(supportless_fdm_recipe_from_suggestion)
        .collect()
}

pub fn reorient_ecky_source(source: &str, rotation_degrees: [i16; 3]) -> Result<String, String> {
    let model_open = find_top_level_form(source, "model").ok_or_else(|| {
        "source-consistent reorient preview requires a top-level `(model ...)` form.".to_string()
    })?;
    let model_close = matching_paren(source, model_open).ok_or_else(|| {
        "source-consistent reorient preview found an unterminated `(model ...)` form.".to_string()
    })?;
    let elements = list_elements(source, model_open, model_close)?;
    let mut replacements = Vec::new();

    for element in elements.iter().skip(1) {
        if !element.is_list {
            continue;
        }
        if list_head(source, element.start, element.end - 1)? != Some("part") {
            continue;
        }
        let part_elements = list_elements(source, element.start, element.end - 1)?;
        if part_elements.len() != 3 {
            return Err(
                "source-consistent reorient preview only supports `(part name geometry)` roots."
                    .to_string(),
            );
        }
        let geometry = &part_elements[2];
        replacements.push((
            geometry.start,
            geometry.end,
            format!(
                "(rotate {} {} {} {})",
                rotation_degrees[0],
                rotation_degrees[1],
                rotation_degrees[2],
                &source[geometry.start..geometry.end]
            ),
        ));
    }

    if replacements.is_empty() {
        return Err(
            "source-consistent reorient preview found no top-level `(part name geometry)` roots."
                .to_string(),
        );
    }

    let mut next = source.to_string();
    for (start, end, replacement) in replacements.into_iter().rev() {
        next.replace_range(start..end, &replacement);
    }
    Ok(next)
}

#[derive(Clone, Debug)]
struct SourceElement {
    start: usize,
    end: usize,
    is_list: bool,
}

fn find_top_level_form(source: &str, head: &str) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        index = skip_ws_and_comments(source, index);
        if index >= bytes.len() {
            return None;
        }
        if bytes[index] != b'(' {
            return None;
        }
        let close = matching_paren(source, index)?;
        if list_head(source, index, close).ok().flatten() == Some(head) {
            return Some(index);
        }
        index = close + 1;
    }
    None
}

fn list_head(source: &str, open: usize, close: usize) -> Result<Option<&str>, String> {
    let mut index = skip_ws_and_comments(source, open + 1);
    if index >= close {
        return Ok(None);
    }
    if source.as_bytes()[index] == b'(' {
        return Ok(None);
    }
    let start = index;
    index = read_atom_end(source, index);
    Ok(Some(&source[start..index]))
}

fn list_elements(source: &str, open: usize, close: usize) -> Result<Vec<SourceElement>, String> {
    let mut elements = Vec::new();
    let mut index = open + 1;
    while index < close {
        index = skip_ws_and_comments(source, index);
        if index >= close {
            break;
        }
        let start = index;
        let bytes = source.as_bytes();
        let (end, is_list) = match bytes[index] {
            b'\'' => {
                index += 1;
                index = skip_ws_and_comments(source, index);
                if index < close && bytes[index] == b'(' {
                    let nested_close = matching_paren(source, index)
                        .ok_or_else(|| "unterminated quoted list in Ecky source.".to_string())?;
                    (nested_close + 1, false)
                } else {
                    (read_atom_end(source, index), false)
                }
            }
            b'(' => {
                let nested_close = matching_paren(source, index)
                    .ok_or_else(|| "unterminated list in Ecky source.".to_string())?;
                (nested_close + 1, true)
            }
            _ => (read_atom_end(source, index), false),
        };
        elements.push(SourceElement {
            start,
            end,
            is_list,
        });
        index = end;
    }
    Ok(elements)
}

fn read_atom_end(source: &str, mut index: usize) -> usize {
    let bytes = source.as_bytes();
    while index < bytes.len()
        && !bytes[index].is_ascii_whitespace()
        && bytes[index] != b'('
        && bytes[index] != b')'
        && bytes[index] != b';'
    {
        index += 1;
    }
    index
}

fn skip_ws_and_comments(source: &str, mut index: usize) -> usize {
    let bytes = source.as_bytes();
    while index < bytes.len() {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index < bytes.len() && bytes[index] == b';' {
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        break;
    }
    index
}

fn matching_paren(source: &str, open: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    if bytes.get(open).copied() != Some(b'(') {
        return None;
    }
    let mut depth = 0usize;
    let mut index = open;
    let mut in_string = false;
    let mut escaped = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if byte == b';' {
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if byte == b'"' {
            in_string = true;
        } else if byte == b'(' {
            depth += 1;
        } else if byte == b')' {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index);
            }
        }
        index += 1;
    }
    None
}

fn supportless_fdm_recipe_from_suggestion(
    suggestion: &PrintabilityTransformSuggestion,
) -> Option<SupportlessFdmTransformRecipe> {
    let action_kind = match suggestion.kind {
        PrintabilityTransformSuggestionKind::OrientationHint => {
            let before = suggestion.overhang_ratio_before?;
            let after = suggestion.overhang_ratio_after?;
            if after + f64::EPSILON >= before {
                return None;
            }
            SupportlessFdmRecipeActionKind::Reorient
        }
        PrintabilityTransformSuggestionKind::Chamfer => {
            let before = suggestion.overhang_ratio_before?;
            if before <= CHAMFER_OVERHANG_RATIO_THRESHOLD {
                return None;
            }
            SupportlessFdmRecipeActionKind::Relief
        }
        PrintabilityTransformSuggestionKind::Split => SupportlessFdmRecipeActionKind::Clearance,
    };

    let rationale = suggestion
        .reason
        .clone()
        .unwrap_or_else(|| suggestion.label.clone());
    Some(SupportlessFdmTransformRecipe {
        recipe_id: format!("supportless-fdm-{}", suggestion.suggestion_id),
        action_kind,
        label: suggestion.label.clone(),
        rationale,
        estimated_effect: TransformRecipeEstimatedEffect {
            summary: supportless_fdm_recipe_effect_summary(suggestion),
            overhang_ratio_before: suggestion.overhang_ratio_before,
            overhang_ratio_after: suggestion.overhang_ratio_after,
        },
        target: supportless_fdm_recipe_target(suggestion),
        source_anchor: suggestion.source_anchor.clone(),
        risk_anchor: suggestion.risk_anchor.clone(),
        preview_support_status: TransformRecipeSupportStatus::Pending,
        apply_support_status: TransformRecipeSupportStatus::Unsupported,
        rotation_degrees: suggestion.rotation_degrees,
        chamfer_mm: suggestion.chamfer_mm,
        split_axis: suggestion.split_axis,
        split_position_mm: suggestion.split_position_mm,
        bridge_span_mm: suggestion.bridge_span_mm,
        thin_wall_mm: suggestion.thin_wall_mm,
        unsupported_island_count: suggestion.unsupported_island_count,
    })
}

fn supportless_fdm_recipe_effect_summary(suggestion: &PrintabilityTransformSuggestion) -> String {
    match suggestion.kind {
        PrintabilityTransformSuggestionKind::OrientationHint => match (
            suggestion.overhang_ratio_before,
            suggestion.overhang_ratio_after,
        ) {
            (Some(before), Some(after)) => {
                format!("estimated overhang ratio {before:.3} -> {after:.3}")
            }
            _ => "estimated overhang reduction from reorientation".to_string(),
        },
        PrintabilityTransformSuggestionKind::Chamfer => {
            let chamfer = suggestion.chamfer_mm.unwrap_or(DEFAULT_CHAMFER_MM);
            format!("add {chamfer:.3} mm chamfers to reduce unsupported lower edges")
        }
        PrintabilityTransformSuggestionKind::Split => {
            "split candidate may isolate unsupported or hard-to-orient regions".to_string()
        }
    }
}

fn supportless_fdm_recipe_target(suggestion: &PrintabilityTransformSuggestion) -> Option<String> {
    match suggestion.kind {
        PrintabilityTransformSuggestionKind::OrientationHint => {
            suggestion.target_orientation_label.clone()
        }
        PrintabilityTransformSuggestionKind::Chamfer => Some("unsupportedLowerEdges".to_string()),
        PrintabilityTransformSuggestionKind::Split => suggestion
            .split_axis
            .map(|axis| format!("splitAxis:{axis:?}"))
            .or_else(|| Some("splitCandidate".to_string())),
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct StlVertex([u32; 3]);

#[derive(Clone, Copy, Debug)]
struct IndexedTriangle {
    vertices: [StlVertex; 3],
    coords: [[f32; 3]; 3],
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct StlEdge {
    a: StlVertex,
    b: StlVertex,
}

impl StlVertex {
    fn new(coords: [f32; 3]) -> Self {
        Self(coords.map(stl_float_key))
    }
}

impl IndexedTriangle {
    fn edges(&self) -> [StlEdge; 3] {
        [
            StlEdge::new(self.vertices[0], self.vertices[1]),
            StlEdge::new(self.vertices[1], self.vertices[2]),
            StlEdge::new(self.vertices[2], self.vertices[0]),
        ]
    }
}

impl StlEdge {
    fn new(a: StlVertex, b: StlVertex) -> Self {
        if a <= b {
            Self { a, b }
        } else {
            Self { a: b, b: a }
        }
    }
}

fn parse_stl_triangles(bytes: &[u8]) -> Result<Vec<[[f32; 3]; 3]>, PrintabilityError> {
    if bytes.len() >= 84 {
        let triangle_count = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]);
        let expected_binary_len = (triangle_count as usize)
            .checked_mul(50)
            .and_then(|triangle_bytes| triangle_bytes.checked_add(84));
        if expected_binary_len == Some(bytes.len()) {
            return Ok(parse_binary_stl_triangles(bytes, triangle_count as usize));
        }
    }

    let Some(first_non_whitespace) = bytes.iter().position(|b| !b.is_ascii_whitespace()) else {
        return Err(PrintabilityError::UnreadableStl);
    };
    let stl_body = &bytes[first_non_whitespace..];
    if !stl_body
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"solid"))
    {
        return Err(PrintabilityError::UnreadableStl);
    }
    let text = std::str::from_utf8(stl_body).map_err(|_| PrintabilityError::UnreadableStl)?;
    parse_ascii_stl_triangles(text)
}

fn parse_binary_stl_triangles(bytes: &[u8], triangle_count: usize) -> Vec<[[f32; 3]; 3]> {
    let mut triangles = Vec::with_capacity(triangle_count);
    let mut offset = 84;
    for _ in 0..triangle_count {
        offset += 12;
        let mut coords = [[0.0_f32; 3]; 3];
        for vertex in &mut coords {
            let x = f32::from_le_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
            ]);
            offset += 4;
            let y = f32::from_le_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
            ]);
            offset += 4;
            let z = f32::from_le_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
            ]);
            offset += 4;
            *vertex = [x, y, z];
        }
        offset += 2;
        triangles.push(coords);
    }
    triangles
}

fn parse_ascii_stl_triangles(text: &str) -> Result<Vec<[[f32; 3]; 3]>, PrintabilityError> {
    let facet_count = ascii_stl_facet_count(text);
    if facet_count == 0 {
        return Ok(Vec::new());
    }

    let mut triangles = Vec::with_capacity(facet_count);
    let mut current_vertices: Option<Vec<[f32; 3]>> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if starts_ascii_case(trimmed, "facet") {
            if current_vertices.is_some() {
                return Err(PrintabilityError::UnreadableStl);
            }
            current_vertices = Some(Vec::with_capacity(3));
        } else if starts_ascii_case(trimmed, "vertex") {
            let Some(vertices) = current_vertices.as_mut() else {
                continue;
            };
            let Some(vertex) = parse_ascii_stl_vertex(trimmed) else {
                return Err(PrintabilityError::UnreadableStl);
            };
            vertices.push(vertex);
        } else if starts_ascii_case(trimmed, "endfacet") {
            let Some(vertices) = current_vertices.take() else {
                continue;
            };
            let Ok(coords) = <Vec<[f32; 3]> as TryInto<[[f32; 3]; 3]>>::try_into(vertices) else {
                return Err(PrintabilityError::UnreadableStl);
            };
            triangles.push(coords);
        }
    }

    if current_vertices.is_some() || triangles.len() != facet_count {
        return Err(PrintabilityError::UnreadableStl);
    }

    Ok(triangles)
}

fn parse_ascii_stl_vertex(line: &str) -> Option<[f32; 3]> {
    let mut parts = line.split_whitespace();
    let label = parts.next()?;
    if !label.eq_ignore_ascii_case("vertex") {
        return None;
    }
    let x = parts.next()?.parse::<f32>().ok()?;
    let y = parts.next()?.parse::<f32>().ok()?;
    let z = parts.next()?.parse::<f32>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some([x, y, z])
}

fn ascii_stl_facet_count(text: &str) -> usize {
    text.lines()
        .filter(|line| {
            line.trim_start()
                .get(..5)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("facet"))
        })
        .count()
}

fn starts_ascii_case(text: &str, prefix: &str) -> bool {
    text.as_bytes()
        .get(..prefix.len())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix.as_bytes()))
}

fn bounding_box(triangles: &[[[f32; 3]; 3]]) -> Option<PrintabilityBoundingBox> {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    let mut has_finite_vertex = false;
    for vertex in triangles.iter().flat_map(|triangle| triangle.iter()) {
        if vertex.iter().any(|coordinate| !coordinate.is_finite()) {
            continue;
        }
        has_finite_vertex = true;
        for axis in 0..3 {
            let coordinate = vertex[axis] as f64;
            min[axis] = min[axis].min(coordinate);
            max[axis] = max[axis].max(coordinate);
        }
    }

    has_finite_vertex.then(|| PrintabilityBoundingBox {
        min,
        max,
        size: [max[0] - min[0], max[1] - min[1], max[2] - min[2]],
    })
}

fn stl_float_key(value: f32) -> u32 {
    if value == 0.0 {
        0.0_f32.to_bits()
    } else if value.is_nan() {
        f32::NAN.to_bits()
    } else {
        value.to_bits()
    }
}

fn stl_edge_triangles(triangles: &[IndexedTriangle]) -> HashMap<StlEdge, Vec<usize>> {
    let mut edge_triangles: HashMap<StlEdge, Vec<usize>> = HashMap::new();
    for (triangle_idx, triangle) in triangles.iter().enumerate() {
        for edge in triangle.edges() {
            edge_triangles.entry(edge).or_default().push(triangle_idx);
        }
    }
    edge_triangles
}

#[derive(Clone, Copy, Debug)]
struct StlComponentMetric {
    triangle_count: usize,
    min_z: f32,
}

fn stl_component_metrics(
    triangles: &[IndexedTriangle],
    edge_triangles: &HashMap<StlEdge, Vec<usize>>,
) -> Vec<StlComponentMetric> {
    let triangle_count = triangles.len();
    if triangle_count == 0 {
        return Vec::new();
    }
    let mut adjacency = vec![Vec::new(); triangle_count];
    for triangle_ids in edge_triangles.values() {
        for (position, &left) in triangle_ids.iter().enumerate() {
            for &right in &triangle_ids[(position + 1)..] {
                if left == right {
                    continue;
                }
                adjacency[left].push(right);
                adjacency[right].push(left);
            }
        }
    }

    let mut visited = vec![false; triangle_count];
    let mut components = Vec::new();
    for start in 0..triangle_count {
        if visited[start] {
            continue;
        }
        visited[start] = true;
        let mut queue = VecDeque::from([start]);
        let mut component_triangle_count = 0usize;
        let mut component_min_z = f32::INFINITY;
        while let Some(current) = queue.pop_front() {
            component_triangle_count += 1;
            if let Some(min_z) = triangles[current]
                .coords
                .iter()
                .map(|point| point[2])
                .filter(|z| z.is_finite())
                .reduce(f32::min)
            {
                component_min_z = component_min_z.min(min_z);
            }
            for &next in &adjacency[current] {
                if visited[next] {
                    continue;
                }
                visited[next] = true;
                queue.push_back(next);
            }
        }
        components.push(StlComponentMetric {
            triangle_count: component_triangle_count,
            min_z: if component_min_z.is_finite() {
                component_min_z
            } else {
                0.0
            },
        });
    }
    components
}

fn stl_overhang_triangle_count(triangles: &[IndexedTriangle]) -> usize {
    let Some(min_z) = stl_min_z(triangles) else {
        return 0;
    };
    triangles
        .iter()
        .filter(|triangle| {
            let centroid_z = triangle.coords.iter().map(|point| point[2]).sum::<f32>() / 3.0;
            if centroid_z <= min_z + BUILD_PLANE_EPSILON_MM {
                return false;
            }
            triangle_unit_normal_z(triangle)
                .is_some_and(|normal_z| normal_z <= OVERHANG_NORMAL_Z_THRESHOLD)
        })
        .count()
}

#[derive(Clone, Copy, Debug)]
struct OrientationRotation {
    label: &'static str,
    rotation_degrees: [i16; 3],
}

#[derive(Clone, Debug)]
struct OrientationCandidateMetrics {
    label: String,
    rotation_degrees: [i16; 3],
    overhang_ratio: f64,
    bbox_height_mm: Option<f64>,
    footprint_area_mm2: Option<f64>,
}

fn orientation_score(triangles: &[IndexedTriangle]) -> OrientationScorePlaceholder {
    if triangles.is_empty() {
        return OrientationScorePlaceholder {
            current_orientation_score: None,
            candidates: Vec::new(),
            status: OrientationScoreStatus::NotScored,
        };
    }

    let metrics: Vec<OrientationCandidateMetrics> = ORIENTATION_ROTATIONS
        .iter()
        .map(|rotation| orientation_candidate_metrics(triangles, *rotation))
        .collect();
    let max_height = metrics
        .iter()
        .filter_map(|candidate| candidate.bbox_height_mm)
        .fold(0.0_f64, f64::max);
    let max_footprint = metrics
        .iter()
        .filter_map(|candidate| candidate.footprint_area_mm2)
        .fold(0.0_f64, f64::max);

    let mut candidates: Vec<OrientationScoreCandidate> = metrics
        .into_iter()
        .map(|candidate| {
            let score = orientation_candidate_score(&candidate, max_height, max_footprint);
            OrientationScoreCandidate {
                label: candidate.label,
                score: Some(score),
                rotation_degrees: candidate.rotation_degrees,
                overhang_ratio: Some(candidate.overhang_ratio),
                bbox_height_mm: candidate.bbox_height_mm,
                footprint_area_mm2: candidate.footprint_area_mm2,
            }
        })
        .collect();
    let current_orientation_score = candidates
        .iter()
        .find(|candidate| candidate.rotation_degrees == [0, 0, 0])
        .and_then(|candidate| candidate.score);

    candidates.sort_by(compare_orientation_candidates);

    OrientationScorePlaceholder {
        current_orientation_score,
        candidates,
        status: OrientationScoreStatus::Scored,
    }
}

const ORIENTATION_ROTATIONS: [OrientationRotation; 6] = [
    OrientationRotation {
        label: "current",
        rotation_degrees: [0, 0, 0],
    },
    OrientationRotation {
        label: "rotateX90",
        rotation_degrees: [90, 0, 0],
    },
    OrientationRotation {
        label: "rotateX180",
        rotation_degrees: [180, 0, 0],
    },
    OrientationRotation {
        label: "rotateX270",
        rotation_degrees: [270, 0, 0],
    },
    OrientationRotation {
        label: "rotateY90",
        rotation_degrees: [0, 90, 0],
    },
    OrientationRotation {
        label: "rotateY270",
        rotation_degrees: [0, 270, 0],
    },
];

fn orientation_candidate_metrics(
    triangles: &[IndexedTriangle],
    rotation: OrientationRotation,
) -> OrientationCandidateMetrics {
    let rotated_triangles: Vec<IndexedTriangle> = triangles
        .iter()
        .map(|triangle| rotate_triangle(*triangle, rotation.rotation_degrees))
        .collect();
    let overhang_triangle_count = stl_overhang_triangle_count(&rotated_triangles);
    let overhang_ratio = overhang_triangle_count as f64 / rotated_triangles.len() as f64;
    let bbox = indexed_bounding_box(&rotated_triangles);

    OrientationCandidateMetrics {
        label: rotation.label.to_string(),
        rotation_degrees: rotation.rotation_degrees,
        overhang_ratio,
        bbox_height_mm: bbox.map(|bbox| bbox.size[2]),
        footprint_area_mm2: bbox.map(|bbox| bbox.size[0] * bbox.size[1]),
    }
}

fn orientation_candidate_score(
    candidate: &OrientationCandidateMetrics,
    max_height: f64,
    max_footprint: f64,
) -> f64 {
    let normalized_height = candidate
        .bbox_height_mm
        .filter(|_| max_height > f64::EPSILON)
        .map(|height| height / max_height)
        .unwrap_or(0.0);
    let normalized_footprint = candidate
        .footprint_area_mm2
        .filter(|_| max_footprint > f64::EPSILON)
        .map(|footprint| footprint / max_footprint)
        .unwrap_or(0.0);

    candidate.overhang_ratio
        + (normalized_height * ORIENTATION_HEIGHT_TIEBREAKER_WEIGHT)
        + (normalized_footprint * ORIENTATION_FOOTPRINT_TIEBREAKER_WEIGHT)
}

fn compare_orientation_candidates(
    left: &OrientationScoreCandidate,
    right: &OrientationScoreCandidate,
) -> std::cmp::Ordering {
    left.overhang_ratio
        .unwrap_or(f64::INFINITY)
        .total_cmp(&right.overhang_ratio.unwrap_or(f64::INFINITY))
        .then_with(|| {
            left.bbox_height_mm
                .unwrap_or(f64::INFINITY)
                .total_cmp(&right.bbox_height_mm.unwrap_or(f64::INFINITY))
        })
        .then_with(|| {
            left.footprint_area_mm2
                .unwrap_or(f64::INFINITY)
                .total_cmp(&right.footprint_area_mm2.unwrap_or(f64::INFINITY))
        })
        .then_with(|| left.label.cmp(&right.label))
}

fn rotate_triangle(triangle: IndexedTriangle, rotation_degrees: [i16; 3]) -> IndexedTriangle {
    let coords = triangle
        .coords
        .map(|point| rotate_point(point, rotation_degrees));
    IndexedTriangle {
        vertices: coords.map(StlVertex::new),
        coords,
    }
}

fn rotate_point([x, y, z]: [f32; 3], rotation_degrees: [i16; 3]) -> [f32; 3] {
    match rotation_degrees {
        [0, 0, 0] => [x, y, z],
        [90, 0, 0] => [x, -z, y],
        [180, 0, 0] => [x, -y, -z],
        [270, 0, 0] => [x, z, -y],
        [0, 90, 0] => [z, y, -x],
        [0, 270, 0] => [-z, y, x],
        _ => [x, y, z],
    }
}

fn indexed_bounding_box(triangles: &[IndexedTriangle]) -> Option<PrintabilityBoundingBox> {
    let coords: Vec<[[f32; 3]; 3]> = triangles.iter().map(|triangle| triangle.coords).collect();
    bounding_box(&coords)
}

fn printability_transform_suggestions(
    overhang_ratio: f64,
    bbox: Option<&PrintabilityBoundingBox>,
    topology: &PrintabilityTopologyFacts,
    risk_metrics: &PrintabilityRiskMetrics,
    orientation_score: &OrientationScorePlaceholder,
) -> Vec<PrintabilityTransformSuggestion> {
    let mut suggestions = Vec::new();
    if let Some(suggestion) = orientation_hint_suggestion(overhang_ratio, orientation_score) {
        suggestions.push(suggestion);
    }
    if overhang_ratio > CHAMFER_OVERHANG_RATIO_THRESHOLD {
        suggestions.push(chamfer_overhang_suggestion(overhang_ratio));
    }
    if let Some(suggestion) = split_risk_suggestion(bbox, topology, risk_metrics) {
        suggestions.push(suggestion);
    }
    suggestions
}

fn orientation_hint_suggestion(
    overhang_ratio: f64,
    orientation_score: &OrientationScorePlaceholder,
) -> Option<PrintabilityTransformSuggestion> {
    let best = orientation_score.candidates.first()?;
    let current_score = orientation_score.current_orientation_score?;
    let best_score = best.score?;
    let confidence = if best_score + f64::EPSILON < current_score {
        0.9
    } else {
        0.65
    };

    Some(PrintabilityTransformSuggestion {
        suggestion_id: "orientation-best".to_string(),
        kind: PrintabilityTransformSuggestionKind::OrientationHint,
        label: "Use best scored orientation".to_string(),
        confidence: Some(confidence),
        reason: Some(format!(
            "best scored orientation '{}' has overhang ratio {:.3} versus current {:.3}",
            best.label,
            best.overhang_ratio.unwrap_or(overhang_ratio),
            overhang_ratio
        )),
        source_anchor: None,
        risk_anchor: None,
        rotation_degrees: Some(best.rotation_degrees),
        target_orientation_label: Some(best.label.clone()),
        overhang_ratio_before: Some(overhang_ratio),
        overhang_ratio_after: best.overhang_ratio,
        chamfer_mm: None,
        split_axis: None,
        split_position_mm: None,
        bridge_span_mm: None,
        thin_wall_mm: None,
        unsupported_island_count: None,
    })
}

fn chamfer_overhang_suggestion(overhang_ratio: f64) -> PrintabilityTransformSuggestion {
    PrintabilityTransformSuggestion {
        suggestion_id: "chamfer-overhangs".to_string(),
        kind: PrintabilityTransformSuggestionKind::Chamfer,
        label: "Chamfer unsupported lower edges".to_string(),
        confidence: Some(0.7),
        reason: Some(format!(
            "overhang ratio {:.3} exceeds advisory threshold {:.3}",
            overhang_ratio, CHAMFER_OVERHANG_RATIO_THRESHOLD
        )),
        source_anchor: None,
        risk_anchor: None,
        rotation_degrees: None,
        target_orientation_label: None,
        overhang_ratio_before: Some(overhang_ratio),
        overhang_ratio_after: None,
        chamfer_mm: Some(DEFAULT_CHAMFER_MM),
        split_axis: None,
        split_position_mm: None,
        bridge_span_mm: None,
        thin_wall_mm: None,
        unsupported_island_count: None,
    }
}

fn split_risk_suggestion(
    bbox: Option<&PrintabilityBoundingBox>,
    topology: &PrintabilityTopologyFacts,
    risk_metrics: &PrintabilityRiskMetrics,
) -> Option<PrintabilityTransformSuggestion> {
    let component_count = topology.component_count.unwrap_or(0);
    let bbox_risk = bbox.and_then(split_bbox_risk);
    let bridge_span_risk = risk_metrics
        .bridge_span_mm
        .filter(|span| *span >= BRIDGE_SPAN_RISK_THRESHOLD_MM);
    let thin_wall_risk = risk_metrics
        .thin_wall_mm
        .filter(|wall| *wall <= THIN_WALL_RISK_THRESHOLD_MM);
    let unsupported_island_risk = risk_metrics
        .unsupported_island_count
        .filter(|count| *count > 0);
    if component_count <= 1
        && bbox_risk.is_none()
        && bridge_span_risk.is_none()
        && thin_wall_risk.is_none()
        && unsupported_island_risk.is_none()
    {
        return None;
    }

    let (axis, position, aspect_ratio) = bbox_risk
        .or_else(|| bbox.and_then(|bbox| split_axis_and_position(bbox).map(|axis| (axis, None))))
        .map(|((axis, position), aspect_ratio)| (Some(axis), Some(position), aspect_ratio))
        .unwrap_or((None, None, None));
    let mut reasons = Vec::new();
    if component_count > 1 {
        reasons.push(format!("{component_count} disconnected components"));
    }
    if let Some(aspect_ratio) = aspect_ratio {
        reasons.push(format!("bbox aspect ratio {aspect_ratio:.2}"));
    }
    if let Some(bridge_span_mm) = bridge_span_risk {
        reasons.push(format!(
            "bridge span {bridge_span_mm:.2} mm exceeds {:.2} mm advisory",
            BRIDGE_SPAN_RISK_THRESHOLD_MM
        ));
    }
    if let Some(thin_wall_mm) = thin_wall_risk {
        reasons.push(format!(
            "thin wall {thin_wall_mm:.2} mm below {:.2} mm advisory",
            THIN_WALL_RISK_THRESHOLD_MM
        ));
    }
    if let Some(unsupported_island_count) = unsupported_island_risk {
        reasons.push(format!(
            "{unsupported_island_count} small unsupported islands detected"
        ));
    }

    Some(PrintabilityTransformSuggestion {
        suggestion_id: "split-risk".to_string(),
        kind: PrintabilityTransformSuggestionKind::Split,
        label: "Consider splitting model for printing".to_string(),
        confidence: Some(if component_count > 1 {
            0.85
        } else if unsupported_island_risk.is_some() {
            0.82
        } else if thin_wall_risk.is_some() {
            0.8
        } else if bridge_span_risk.is_some() {
            0.76
        } else {
            0.72
        }),
        reason: Some(reasons.join("; ")),
        source_anchor: None,
        risk_anchor: None,
        rotation_degrees: None,
        target_orientation_label: None,
        overhang_ratio_before: None,
        overhang_ratio_after: None,
        chamfer_mm: None,
        split_axis: axis,
        split_position_mm: position,
        bridge_span_mm: risk_metrics.bridge_span_mm,
        thin_wall_mm: risk_metrics.thin_wall_mm,
        unsupported_island_count: risk_metrics.unsupported_island_count,
    })
}

fn printability_risk_metrics(
    bbox: Option<&PrintabilityBoundingBox>,
    component_metrics: &[StlComponentMetric],
) -> PrintabilityRiskMetrics {
    let (bridge_span_mm, thin_wall_mm) = if let Some(bbox) = bbox {
        let bridge_span_mm = bbox
            .size
            .iter()
            .take(2)
            .copied()
            .filter(|size| size.is_finite() && *size > f64::EPSILON)
            .max_by(|left, right| left.total_cmp(right));
        let thin_wall_mm = bbox
            .size
            .iter()
            .copied()
            .filter(|size| size.is_finite() && *size > f64::EPSILON)
            .min_by(|left, right| left.total_cmp(right));
        (bridge_span_mm, thin_wall_mm)
    } else {
        (None, None)
    };
    let unsupported_island_count = unsupported_island_count(component_metrics);

    PrintabilityRiskMetrics {
        bridge_span_mm,
        thin_wall_mm,
        unsupported_island_count,
    }
}

fn unsupported_island_count(component_metrics: &[StlComponentMetric]) -> Option<u32> {
    let build_plane_z = component_metrics
        .iter()
        .map(|metric| metric.min_z)
        .filter(|z| z.is_finite())
        .reduce(f32::min)?;
    let count = component_metrics
        .iter()
        .filter(|metric| {
            metric.triangle_count <= UNSUPPORTED_ISLAND_TRIANGLE_MAX
                && metric.min_z > build_plane_z + UNSUPPORTED_ISLAND_MIN_CLEARANCE_MM
        })
        .count();
    (count > 0).then(|| usize_metric(count))
}

fn split_bbox_risk(
    bbox: &PrintabilityBoundingBox,
) -> Option<((PrintabilityAxis, f64), Option<f64>)> {
    let (axis, position) = split_axis_and_position(bbox)?;
    let min_size = bbox
        .size
        .iter()
        .copied()
        .filter(|size| size.is_finite() && *size > f64::EPSILON)
        .fold(f64::INFINITY, f64::min);
    let max_size = bbox
        .size
        .iter()
        .copied()
        .filter(|size| size.is_finite())
        .fold(0.0_f64, f64::max);
    if !min_size.is_finite() || max_size / min_size < SPLIT_ASPECT_RATIO_THRESHOLD {
        return None;
    }
    Some(((axis, position), Some(max_size / min_size)))
}

fn split_axis_and_position(bbox: &PrintabilityBoundingBox) -> Option<(PrintabilityAxis, f64)> {
    let (axis_idx, _) = bbox
        .size
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, size)| size.is_finite())
        .max_by(|left, right| left.1.total_cmp(&right.1))?;
    let axis = match axis_idx {
        0 => PrintabilityAxis::X,
        1 => PrintabilityAxis::Y,
        2 => PrintabilityAxis::Z,
        _ => return None,
    };
    Some((axis, (bbox.min[axis_idx] + bbox.max[axis_idx]) / 2.0))
}

fn stl_min_z(triangles: &[IndexedTriangle]) -> Option<f32> {
    triangles
        .iter()
        .flat_map(|triangle| triangle.coords.iter().map(|point| point[2]))
        .filter(|z| z.is_finite())
        .reduce(f32::min)
}

fn triangle_unit_normal_z(triangle: &IndexedTriangle) -> Option<f32> {
    let [a, b, c] = triangle.coords;
    let ux = b[0] - a[0];
    let uy = b[1] - a[1];
    let uz = b[2] - a[2];
    let vx = c[0] - a[0];
    let vy = c[1] - a[1];
    let vz = c[2] - a[2];
    let nx = uy * vz - uz * vy;
    let ny = uz * vx - ux * vz;
    let nz = ux * vy - uy * vx;
    let len = (nx * nx + ny * ny + nz * nz).sqrt();
    if !len.is_finite() || len <= f32::EPSILON {
        return None;
    }
    Some(nz / len)
}

fn usize_metric(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    #[test]
    fn parsed_closed_tetra_reports_bbox_and_manifold_single_component() {
        let result = analyze_stl_triangles(&closed_tetra_triangles(0.0));

        assert_eq!(result.triangle_count, 4);
        assert_eq!(result.overhang_triangle_count, 0);
        assert_eq!(result.overhang_ratio, 0.0);
        assert_eq!(result.topology.component_count, Some(1));
        assert_eq!(result.topology.non_manifold_edge_count, Some(0));
        assert_eq!(
            result.bbox,
            Some(PrintabilityBoundingBox {
                min: [0.0, 0.0, 0.0],
                max: [1.0, 1.0, 1.0],
                size: [1.0, 1.0, 1.0],
            })
        );
        assert_eq!(
            result.orientation_score.status,
            OrientationScoreStatus::Scored
        );
        assert_eq!(result.orientation_score.candidates.len(), 6);
        assert!(result
            .orientation_score
            .current_orientation_score
            .is_some_and(|score| score.is_finite()));
    }

    #[test]
    fn parsed_raised_downward_triangle_reports_overhang_ratio() {
        let triangles = [
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            [[0.0, 0.0, 2.0], [0.0, 1.0, 2.0], [1.0, 0.0, 2.0]],
        ];

        let result = analyze_stl_triangles(&triangles);

        assert_eq!(result.triangle_count, 2);
        assert_eq!(result.overhang_triangle_count, 1);
        assert_eq!(result.overhang_ratio, 0.5);
        assert_eq!(result.topology.component_count, Some(2));
        assert_eq!(result.topology.non_manifold_edge_count, Some(6));
    }

    #[test]
    fn orientation_score_ranks_lower_overhang_fixed_rotation_first() {
        let triangles = [
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            [[0.0, 0.0, 2.0], [0.0, 1.0, 2.0], [1.0, 0.0, 2.0]],
        ];

        let result = analyze_stl_triangles(&triangles);

        let current_score = result
            .orientation_score
            .current_orientation_score
            .expect("current orientation score");
        let current_candidate = result
            .orientation_score
            .candidates
            .iter()
            .find(|candidate| candidate.label == "current")
            .expect("current candidate");
        let best_candidate = result
            .orientation_score
            .candidates
            .first()
            .expect("best candidate");

        assert_eq!(
            result.orientation_score.status,
            OrientationScoreStatus::Scored
        );
        assert_eq!(current_candidate.overhang_ratio, Some(0.5));
        assert_eq!(current_candidate.score, Some(current_score));
        assert_eq!(best_candidate.label, "rotateX270");
        assert_eq!(best_candidate.rotation_degrees, [270, 0, 0]);
        assert_eq!(best_candidate.overhang_ratio, Some(0.0));
        assert!(best_candidate.score.expect("best score") < current_score);
    }

    #[test]
    fn orientation_score_uses_bbox_height_before_footprint_as_tiebreaker() {
        let result = analyze_stl_triangles(&box_triangles([0.0, 0.0, 0.0], [1.0, 2.0, 3.0]));

        let best_candidate = result
            .orientation_score
            .candidates
            .first()
            .expect("best candidate");
        let current_candidate = result
            .orientation_score
            .candidates
            .iter()
            .find(|candidate| candidate.label == "current")
            .expect("current candidate");

        assert_eq!(best_candidate.overhang_ratio, Some(0.0));
        assert_eq!(best_candidate.bbox_height_mm, Some(1.0));
        assert_eq!(best_candidate.footprint_area_mm2, Some(6.0));
        assert_eq!(current_candidate.overhang_ratio, Some(0.0));
        assert_eq!(current_candidate.bbox_height_mm, Some(3.0));
        assert!(
            best_candidate.score.expect("best score")
                < current_candidate.score.expect("current score")
        );
    }

    #[test]
    fn printability_suggestions_include_best_orientation_hint() {
        let triangles = [
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            [[0.0, 0.0, 2.0], [0.0, 1.0, 2.0], [1.0, 0.0, 2.0]],
        ];

        let result = analyze_stl_triangles(&triangles);
        let suggestion = result
            .transform_suggestions
            .iter()
            .find(|suggestion| {
                suggestion.kind == PrintabilityTransformSuggestionKind::OrientationHint
            })
            .expect("orientation suggestion");

        assert_eq!(suggestion.suggestion_id, "orientation-best");
        assert_eq!(
            suggestion.target_orientation_label.as_deref(),
            Some("rotateX270")
        );
        assert_eq!(suggestion.rotation_degrees, Some([270, 0, 0]));
        assert_eq!(suggestion.overhang_ratio_before, Some(0.5));
        assert_eq!(suggestion.overhang_ratio_after, Some(0.0));
        assert!(suggestion
            .confidence
            .is_some_and(|confidence| confidence >= 0.8));
        assert!(suggestion
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("best scored orientation")));
        assert_eq!(suggestion.source_anchor, None);
    }

    #[test]
    fn printability_suggestions_include_chamfer_when_overhang_ratio_exceeds_threshold() {
        let triangles = [
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            [[0.0, 0.0, 2.0], [0.0, 1.0, 2.0], [1.0, 0.0, 2.0]],
        ];

        let result = analyze_stl_triangles(&triangles);
        let suggestion = result
            .transform_suggestions
            .iter()
            .find(|suggestion| suggestion.kind == PrintabilityTransformSuggestionKind::Chamfer)
            .expect("chamfer suggestion");

        assert_eq!(suggestion.suggestion_id, "chamfer-overhangs");
        assert_eq!(suggestion.chamfer_mm, Some(0.5));
        assert!(suggestion
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("overhang ratio 0.500")));
        assert_eq!(suggestion.source_anchor, None);
    }

    #[test]
    fn printability_suggestions_include_split_when_component_facts_show_risk() {
        let mut triangles = closed_tetra_triangles(0.0);
        triangles.extend(closed_tetra_triangles(3.0));

        let result = analyze_stl_triangles(&triangles);
        let suggestion = result
            .transform_suggestions
            .iter()
            .find(|suggestion| suggestion.kind == PrintabilityTransformSuggestionKind::Split)
            .expect("split suggestion");

        assert_eq!(suggestion.suggestion_id, "split-risk");
        assert_eq!(suggestion.split_axis, Some(PrintabilityAxis::X));
        assert_eq!(suggestion.split_position_mm, Some(2.0));
        assert!(suggestion
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("2 disconnected components")));
        assert_eq!(suggestion.source_anchor, None);
    }

    #[test]
    fn printability_suggestions_include_split_when_bbox_aspect_ratio_shows_risk() {
        let result = analyze_stl_triangles(&box_triangles([0.0, 0.0, 0.0], [10.0, 1.0, 1.0]));
        let suggestion = result
            .transform_suggestions
            .iter()
            .find(|suggestion| suggestion.kind == PrintabilityTransformSuggestionKind::Split)
            .expect("split suggestion");

        assert_eq!(suggestion.split_axis, Some(PrintabilityAxis::X));
        assert_eq!(suggestion.split_position_mm, Some(5.0));
        assert!(suggestion
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("bbox aspect ratio 10.00")));
    }

    #[test]
    fn printability_detects_bridge_span_metric_and_triggers_clearance_recipe() {
        let result = analyze_stl_triangles(&box_triangles([0.0, 0.0, 0.0], [21.0, 20.0, 10.0]));
        assert_eq!(result.risk_metrics.bridge_span_mm, Some(21.0));
        assert_eq!(result.risk_metrics.thin_wall_mm, Some(10.0));

        let suggestion = result
            .transform_suggestions
            .iter()
            .find(|suggestion| suggestion.kind == PrintabilityTransformSuggestionKind::Split)
            .expect("split suggestion");
        assert_eq!(suggestion.bridge_span_mm, Some(21.0));
        assert_eq!(suggestion.thin_wall_mm, Some(10.0));
        assert!(suggestion.reason.as_deref().is_some_and(
            |reason| reason.contains("bridge span 21.00 mm exceeds 20.00 mm advisory")
        ));

        let recipe = supportless_fdm_transform_recipes(&result)
            .into_iter()
            .find(|recipe| recipe.action_kind == SupportlessFdmRecipeActionKind::Clearance)
            .expect("clearance recipe");
        assert_eq!(recipe.bridge_span_mm, Some(21.0));
        assert_eq!(recipe.thin_wall_mm, Some(10.0));
    }

    #[test]
    fn printability_detects_thin_wall_metric_and_triggers_clearance_recipe() {
        let result = analyze_stl_triangles(&box_triangles([0.0, 0.0, 0.0], [4.0, 4.0, 1.0]));
        assert_eq!(result.risk_metrics.bridge_span_mm, Some(4.0));
        assert_eq!(result.risk_metrics.thin_wall_mm, Some(1.0));

        let suggestion = result
            .transform_suggestions
            .iter()
            .find(|suggestion| suggestion.kind == PrintabilityTransformSuggestionKind::Split)
            .expect("split suggestion");
        assert_eq!(suggestion.bridge_span_mm, Some(4.0));
        assert_eq!(suggestion.thin_wall_mm, Some(1.0));
        assert!(suggestion
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("thin wall 1.00 mm below 1.20 mm advisory")));

        let recipe = supportless_fdm_transform_recipes(&result)
            .into_iter()
            .find(|recipe| recipe.action_kind == SupportlessFdmRecipeActionKind::Clearance)
            .expect("clearance recipe");
        assert_eq!(recipe.bridge_span_mm, Some(4.0));
        assert_eq!(recipe.thin_wall_mm, Some(1.0));
    }

    #[test]
    fn printability_detects_small_unsupported_island_and_triggers_clearance_recipe() {
        let mut triangles = box_triangles([0.0, 0.0, 0.0], [6.0, 6.0, 2.0]);
        let floating_island = offset_triangles(&closed_tetra_triangles(0.0), [10.0, 0.0, 5.0]);
        triangles.extend(floating_island);

        let result = analyze_stl_triangles(&triangles);
        assert_eq!(result.risk_metrics.unsupported_island_count, Some(1));

        let suggestion = result
            .transform_suggestions
            .iter()
            .find(|suggestion| suggestion.kind == PrintabilityTransformSuggestionKind::Split)
            .expect("split suggestion");
        assert_eq!(suggestion.unsupported_island_count, Some(1));
        assert!(suggestion
            .reason
            .as_deref()
            .is_some_and(|reason| { reason.contains("1 small unsupported islands detected") }));

        let recipe = supportless_fdm_transform_recipes(&result)
            .into_iter()
            .find(|recipe| recipe.action_kind == SupportlessFdmRecipeActionKind::Clearance)
            .expect("clearance recipe");
        assert_eq!(recipe.unsupported_island_count, Some(1));
    }

    #[test]
    fn printability_ignores_large_floating_component_for_unsupported_island_metric() {
        let mut triangles = box_triangles([0.0, 0.0, 0.0], [6.0, 6.0, 2.0]);
        let floating_large_component = box_triangles([10.0, 0.0, 5.0], [14.0, 4.0, 9.0]);
        triangles.extend(floating_large_component);

        let result = analyze_stl_triangles(&triangles);
        assert_eq!(result.risk_metrics.unsupported_island_count, None);

        let suggestion = result
            .transform_suggestions
            .iter()
            .find(|suggestion| suggestion.kind == PrintabilityTransformSuggestionKind::Split)
            .expect("split suggestion");
        assert_eq!(suggestion.unsupported_island_count, None);
        assert!(suggestion
            .reason
            .as_deref()
            .is_some_and(|reason| { !reason.contains("small unsupported islands detected") }));
    }

    #[test]
    fn source_anchor_enrichment_sets_missing_transform_suggestion_anchors() {
        let mut result = analyze_stl_triangles(&[
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            [[0.0, 0.0, 2.0], [0.0, 1.0, 2.0], [1.0, 0.0, 2.0]],
        ]);

        enrich_transform_suggestions_with_source_anchor(
            &mut result,
            Some("feature:feature-ledge".to_string()),
        );

        assert!(
            !result.transform_suggestions.is_empty(),
            "expected suggestions for overhang mesh"
        );
        assert!(result.transform_suggestions.iter().all(|suggestion| {
            suggestion.source_anchor.as_deref() == Some("feature:feature-ledge")
        }));
    }

    #[test]
    fn source_anchor_enrichment_preserves_none_without_clear_anchor() {
        let mut result = analyze_stl_triangles(&[
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            [[0.0, 0.0, 2.0], [0.0, 1.0, 2.0], [1.0, 0.0, 2.0]],
        ]);

        enrich_transform_suggestions_with_source_anchor(&mut result, None);

        assert!(
            !result.transform_suggestions.is_empty(),
            "expected suggestions for overhang mesh"
        );
        assert!(result
            .transform_suggestions
            .iter()
            .all(|suggestion| suggestion.source_anchor.is_none()));
    }

    #[test]
    fn supportless_fdm_recipes_include_overhang_reduction_actions() {
        let mut result = analyze_stl_triangles(&[
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            [[0.0, 0.0, 2.0], [0.0, 1.0, 2.0], [1.0, 0.0, 2.0]],
        ]);
        enrich_transform_suggestions_with_source_anchor(
            &mut result,
            Some("feature:feature-ledge".to_string()),
        );

        let recipes = supportless_fdm_transform_recipes(&result);

        let recipe = recipes
            .iter()
            .find(|recipe| recipe.action_kind == SupportlessFdmRecipeActionKind::Reorient)
            .expect("reorient recipe");
        assert_eq!(recipe.recipe_id, "supportless-fdm-orientation-best");
        assert_eq!(
            recipe.source_anchor.as_deref(),
            Some("feature:feature-ledge")
        );
        assert_eq!(recipe.target.as_deref(), Some("rotateX270"));
        assert_eq!(recipe.rotation_degrees, Some([270, 0, 0]));
        assert_eq!(recipe.estimated_effect.overhang_ratio_before, Some(0.5));
        assert_eq!(recipe.estimated_effect.overhang_ratio_after, Some(0.0));
        assert_eq!(
            recipe.preview_support_status,
            TransformRecipeSupportStatus::Pending
        );
        assert_eq!(
            recipe.apply_support_status,
            TransformRecipeSupportStatus::Unsupported
        );
        assert!(recipe.rationale.contains("best scored orientation"));
    }

    #[test]
    fn supportless_fdm_recipes_include_relief_for_overhang_risk() {
        let result = analyze_stl_triangles(&[
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            [[0.0, 0.0, 2.0], [0.0, 1.0, 2.0], [1.0, 0.0, 2.0]],
        ]);

        let recipes = supportless_fdm_transform_recipes(&result);
        let relief = recipes
            .iter()
            .find(|recipe| recipe.action_kind == SupportlessFdmRecipeActionKind::Relief)
            .expect("relief recipe");

        assert_eq!(relief.recipe_id, "supportless-fdm-chamfer-overhangs");
        assert_eq!(relief.target.as_deref(), Some("unsupportedLowerEdges"));
        assert_eq!(relief.chamfer_mm, Some(0.5));
        assert_eq!(relief.estimated_effect.overhang_ratio_before, Some(0.5));
    }

    #[test]
    fn supportless_fdm_recipes_include_clearance_for_split_risk() {
        let mut triangles = closed_tetra_triangles(0.0);
        triangles.extend(closed_tetra_triangles(3.0));

        let result = analyze_stl_triangles(&triangles);
        let recipes = supportless_fdm_transform_recipes(&result);
        let clearance = recipes
            .iter()
            .find(|recipe| recipe.action_kind == SupportlessFdmRecipeActionKind::Clearance)
            .expect("clearance recipe");

        assert_eq!(clearance.recipe_id, "supportless-fdm-split-risk");
        assert_eq!(clearance.target.as_deref(), Some("splitAxis:X"));
        assert_eq!(clearance.split_axis, Some(PrintabilityAxis::X));
        assert_eq!(clearance.split_position_mm, Some(2.0));
    }

    #[test]
    fn supportless_fdm_recipes_empty_for_no_risk_mesh() {
        // A unit tetra reads as a 1.00 mm thin wall (below the 1.20 mm
        // advisory), so scale it up to make the mesh genuinely risk-free.
        let triangles = closed_tetra_triangles(0.0)
            .into_iter()
            .map(|tri| tri.map(|vertex| vertex.map(|coordinate| coordinate * 10.0)))
            .collect::<Vec<_>>();
        let result = analyze_stl_triangles(&triangles);

        let recipes = supportless_fdm_transform_recipes(&result);

        assert!(recipes.is_empty(), "unexpected recipes: {recipes:?}");
    }

    #[test]
    fn binary_stl_path_reads_tiny_synthetic_mesh() {
        let dir = temp_dir("binary_path");
        let path = dir.join("preview.stl");
        write_binary_stl(&path, &closed_tetra_triangles(0.0));

        let result = analyze_stl_path(&path).expect("printability analysis");

        assert_eq!(result.triangle_count, 4);
        assert_eq!(result.topology.component_count, Some(1));
        assert_eq!(result.topology.non_manifold_edge_count, Some(0));
        fs::remove_dir_all(&dir).ok();
    }

    fn closed_tetra_triangles(x_offset: f32) -> Vec<[[f32; 3]; 3]> {
        let a = [x_offset, 0.0, 0.0];
        let b = [x_offset + 1.0, 0.0, 0.0];
        let c = [x_offset, 1.0, 0.0];
        let d = [x_offset, 0.0, 1.0];
        vec![[a, b, c], [a, d, b], [a, c, d], [b, d, c]]
    }

    fn box_triangles(min: [f32; 3], max: [f32; 3]) -> Vec<[[f32; 3]; 3]> {
        let [x0, y0, z0] = min;
        let [x1, y1, z1] = max;
        let nsw = [x0, y0, z0];
        let nse = [x1, y0, z0];
        let nnw = [x0, y1, z0];
        let nne = [x1, y1, z0];
        let tsw = [x0, y0, z1];
        let tse = [x1, y0, z1];
        let tnw = [x0, y1, z1];
        let tne = [x1, y1, z1];

        vec![
            [nsw, nnw, nse],
            [nse, nnw, nne],
            [tsw, tse, tnw],
            [tse, tne, tnw],
            [nsw, nse, tsw],
            [nse, tse, tsw],
            [nnw, tnw, nne],
            [nne, tnw, tne],
            [nsw, tsw, nnw],
            [nnw, tsw, tnw],
            [nse, nne, tse],
            [nne, tne, tse],
        ]
    }

    fn offset_triangles(triangles: &[[[f32; 3]; 3]], offset: [f32; 3]) -> Vec<[[f32; 3]; 3]> {
        triangles
            .iter()
            .map(|triangle| triangle.map(|[x, y, z]| [x + offset[0], y + offset[1], z + offset[2]]))
            .collect()
    }

    fn write_binary_stl(path: &Path, triangles: &[[[f32; 3]; 3]]) {
        let mut file = fs::File::create(path).unwrap();
        file.write_all(&[0u8; 80]).unwrap();
        file.write_all(&(triangles.len() as u32).to_le_bytes())
            .unwrap();
        for triangle in triangles {
            file.write_all(&[0u8; 12]).unwrap();
            for vertex in triangle {
                for coordinate in vertex {
                    file.write_all(&coordinate.to_le_bytes()).unwrap();
                }
            }
            file.write_all(&[0u8; 2]).unwrap();
        }
        file.flush().unwrap();
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ecky-printability-test-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
