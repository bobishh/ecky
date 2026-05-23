use std::collections::BTreeMap;

use crate::contracts::{ViewerEdgePoint, ViewerEdgeTarget, ViewerFaceTarget};
use crate::ecky_core_ir::{
    CoreKeywordArg, CoreKeywordValue, CoreNode, CoreNodeKind, CoreProgram, CoreSelectorPayload,
    CoreSelectorTagDecl, CoreSelectorTagKind,
};
use crate::ecky_ir::edge_ops::{
    parse_core_edge_selector_payload, parse_core_face_selector_payload, parse_edge_selector_value,
    parse_face_selector_value, EdgeAxis, EdgeBound, EdgeSelector, EdgeSelectorClause, FaceAreaRank,
    FaceSelector, FaceSelectorClause,
};
use crate::models::{
    AppError, AppResult, ModelManifest, SelectionTarget, SelectionTargetKind, TaggedAnchorBinding,
    TaggedAnchorKind,
};

pub(crate) fn resolve_tagged_anchors(
    selector_tags: &[CoreSelectorTagDecl],
    selection_targets: &[SelectionTarget],
    edge_targets: &[ViewerEdgeTarget],
    face_targets: &[ViewerFaceTarget],
) -> AppResult<BTreeMap<String, TaggedAnchorBinding>> {
    let mut tagged_anchors = BTreeMap::new();
    for selector_tag in selector_tags {
        if tagged_anchors.contains_key(selector_tag.name.as_str()) {
            return Err(AppError::validation(format!(
                "manifest got duplicate selector tag '{}'.",
                selector_tag.name
            )));
        }
        let binding = match selector_tag.kind {
            CoreSelectorTagKind::Face => {
                match parse_face_selector_value(&selector_tag.authored_selector)? {
                    FaceSelector::TargetIds(target_ids) => {
                        tagged_anchor_binding_from_selection_targets(
                            selector_tag,
                            &resolve_requested_selection_targets(
                                selector_tag,
                                selection_targets,
                                &target_ids,
                            )?,
                        )
                    }
                    FaceSelector::Clauses(clauses) => tagged_anchor_binding_from_face_targets(
                        selector_tag,
                        &resolve_clause_face_targets(selector_tag, face_targets, &clauses)?,
                    )?,
                }
            }
            CoreSelectorTagKind::Edge => {
                match parse_edge_selector_value(&selector_tag.authored_selector)? {
                    EdgeSelector::TargetIds(target_ids) => {
                        tagged_anchor_binding_from_selection_targets(
                            selector_tag,
                            &resolve_requested_selection_targets(
                                selector_tag,
                                selection_targets,
                                &target_ids,
                            )?,
                        )
                    }
                    selector => tagged_anchor_binding_from_edge_targets(
                        selector_tag,
                        &resolve_clause_edge_targets(selector_tag, edge_targets, &selector)?,
                    )?,
                }
            }
        };
        tagged_anchors.insert(selector_tag.name.clone(), binding);
    }
    Ok(tagged_anchors)
}

fn resolve_requested_selection_targets<'a>(
    selector_tag: &CoreSelectorTagDecl,
    selection_targets: &'a [SelectionTarget],
    requested_target_ids: &[String],
) -> AppResult<Vec<&'a SelectionTarget>> {
    requested_target_ids
        .iter()
        .map(|requested_target_id| {
            selection_targets
                .iter()
                .find(|selection_target| {
                    selection_target.kind == selector_tag_selection_kind(selector_tag.kind)
                        && selection_target_matches_id(selection_target, requested_target_id)
                })
                .ok_or_else(|| {
                    AppError::validation(format!(
                        "tagged anchor '{}' could not resolve targetId '{}'.",
                        selector_tag.name, requested_target_id
                    ))
                })
        })
        .collect()
}

fn resolve_clause_edge_targets<'a>(
    selector_tag: &CoreSelectorTagDecl,
    edge_targets: &'a [ViewerEdgeTarget],
    selector: &EdgeSelector,
) -> AppResult<Vec<&'a ViewerEdgeTarget>> {
    let part_targets = edge_targets
        .iter()
        .filter(|target| {
            topology_target_matches_selector_target(&target.target_id, selector_tag.target.as_str())
        })
        .collect::<Vec<_>>();
    if part_targets.is_empty() {
        return Err(AppError::validation(format!(
            "tagged anchor '{}' matched no edge targets on part '{}'.",
            selector_tag.name, selector_tag.target
        )));
    }
    let Some(clauses) = selector.clauses() else {
        return Ok(part_targets);
    };
    let extrema = edge_target_extrema(&part_targets)?;
    let matching = part_targets
        .into_iter()
        .filter(|target| {
            clauses
                .iter()
                .all(|clause| edge_target_matches_clause(target, *clause, extrema))
        })
        .collect::<Vec<_>>();
    if matching.is_empty() {
        return Err(AppError::validation(format!(
            "tagged anchor '{}' selector '{}' matched no edge targets on part '{}'.",
            selector_tag.name, selector_tag.authored_selector, selector_tag.target
        )));
    }
    Ok(matching)
}

fn resolve_clause_face_targets<'a>(
    selector_tag: &CoreSelectorTagDecl,
    face_targets: &'a [ViewerFaceTarget],
    clauses: &[FaceSelectorClause],
) -> AppResult<Vec<&'a ViewerFaceTarget>> {
    let part_targets = face_targets
        .iter()
        .filter(|target| {
            topology_target_matches_selector_target(&target.target_id, selector_tag.target.as_str())
        })
        .collect::<Vec<_>>();
    if part_targets.is_empty() {
        return Err(AppError::validation(format!(
            "tagged anchor '{}' matched no face targets on part '{}'.",
            selector_tag.name, selector_tag.target
        )));
    }
    if clauses.is_empty() {
        return Ok(part_targets);
    }
    let bounds = face_target_bounds(&part_targets)?;
    let tol = coordinate_tolerance(bounds.span);
    let mut selected = part_targets;
    for clause in clauses {
        selected = match clause {
            FaceSelectorClause::Area(rank) => filter_face_targets_by_area(selected, *rank),
            _ => selected
                .into_iter()
                .filter(|target| face_target_matches_clause(target, *clause, bounds, tol))
                .collect(),
        };
        if selected.is_empty() {
            return Err(AppError::validation(format!(
                "tagged anchor '{}' selector '{}' matched no face targets on part '{}'.",
                selector_tag.name, selector_tag.authored_selector, selector_tag.target
            )));
        }
    }
    Ok(selected)
}

fn tagged_anchor_binding_from_selection_targets(
    selector_tag: &CoreSelectorTagDecl,
    matching_targets: &[&SelectionTarget],
) -> TaggedAnchorBinding {
    let mut target_ids = Vec::new();
    let mut durable_target_ids = Vec::new();
    let mut canonical_target_ids = Vec::new();
    let mut alias_ids = Vec::new();
    for selection_target in matching_targets {
        push_unique_option(&mut target_ids, selection_target.target_id.as_ref());
        push_unique_option(
            &mut durable_target_ids,
            selection_target.durable_target_id.as_ref(),
        );
        push_unique_option(
            &mut canonical_target_ids,
            selection_target.canonical_target_id.as_ref(),
        );
        push_unique_strings(&mut alias_ids, &selection_target.alias_ids);
    }
    TaggedAnchorBinding {
        kind: selector_tag_binding_kind(selector_tag.kind),
        authored_selector: selector_tag.authored_selector.clone(),
        target: selector_tag.target.clone(),
        target_ids,
        durable_target_ids,
        canonical_target_ids,
        alias_ids,
    }
}

fn tagged_anchor_binding_from_edge_targets(
    selector_tag: &CoreSelectorTagDecl,
    matching_targets: &[&ViewerEdgeTarget],
) -> AppResult<TaggedAnchorBinding> {
    tagged_anchor_binding_from_viewer_targets(
        selector_tag,
        matching_targets.iter().map(|target| TaggedViewerTargetRef {
            target_id: target.target_id.as_str(),
            durable_target_id: target.durable_target_id.as_deref(),
            canonical_target_id: target.canonical_target_id.as_deref(),
            alias_ids: target.alias_ids.as_slice(),
        }),
    )
}

fn tagged_anchor_binding_from_face_targets(
    selector_tag: &CoreSelectorTagDecl,
    matching_targets: &[&ViewerFaceTarget],
) -> AppResult<TaggedAnchorBinding> {
    tagged_anchor_binding_from_viewer_targets(
        selector_tag,
        matching_targets.iter().map(|target| TaggedViewerTargetRef {
            target_id: target.target_id.as_str(),
            durable_target_id: target.durable_target_id.as_deref(),
            canonical_target_id: target.canonical_target_id.as_deref(),
            alias_ids: target.alias_ids.as_slice(),
        }),
    )
}

fn tagged_anchor_binding_from_viewer_targets<'a>(
    selector_tag: &CoreSelectorTagDecl,
    matching_targets: impl IntoIterator<Item = TaggedViewerTargetRef<'a>>,
) -> AppResult<TaggedAnchorBinding> {
    let mut target_ids = Vec::new();
    let mut durable_target_ids = Vec::new();
    let mut canonical_target_ids = Vec::new();
    let mut alias_ids = Vec::new();
    for target in matching_targets {
        push_unique_value(&mut target_ids, target.target_id);
        push_unique_str_option(&mut durable_target_ids, target.durable_target_id);
        push_unique_str_option(&mut canonical_target_ids, target.canonical_target_id);
        push_unique_str_slice(&mut alias_ids, target.alias_ids);
    }
    if target_ids.is_empty() {
        return Err(AppError::validation(format!(
            "tagged anchor '{}' recorded no target ids.",
            selector_tag.name
        )));
    }
    Ok(TaggedAnchorBinding {
        kind: selector_tag_binding_kind(selector_tag.kind),
        authored_selector: selector_tag.authored_selector.clone(),
        target: selector_tag.target.clone(),
        target_ids,
        durable_target_ids,
        canonical_target_ids,
        alias_ids,
    })
}

#[derive(Clone, Copy)]
struct TaggedViewerTargetRef<'a> {
    target_id: &'a str,
    durable_target_id: Option<&'a str>,
    canonical_target_id: Option<&'a str>,
    alias_ids: &'a [String],
}

#[derive(Clone, Copy)]
struct EdgeTargetExtrema {
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
    min_z: f64,
    max_z: f64,
    tol: f64,
}

#[derive(Clone, Copy)]
struct FaceTargetBounds {
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
    min_z: f64,
    max_z: f64,
    span: f64,
}

fn selector_tag_binding_kind(kind: CoreSelectorTagKind) -> TaggedAnchorKind {
    match kind {
        CoreSelectorTagKind::Face => TaggedAnchorKind::Face,
        CoreSelectorTagKind::Edge => TaggedAnchorKind::Edge,
    }
}

fn edge_target_extrema(edge_targets: &[&ViewerEdgeTarget]) -> AppResult<EdgeTargetExtrema> {
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut min_z = f64::INFINITY;
    let mut max_z = f64::NEG_INFINITY;
    for target in edge_targets {
        min_x = min_x.min(target.start.x.min(target.end.x));
        max_x = max_x.max(target.start.x.max(target.end.x));
        min_y = min_y.min(target.start.y.min(target.end.y));
        max_y = max_y.max(target.start.y.max(target.end.y));
        min_z = min_z.min(target.start.z.min(target.end.z));
        max_z = max_z.max(target.start.z.max(target.end.z));
    }
    if !min_x.is_finite()
        || !max_x.is_finite()
        || !min_y.is_finite()
        || !max_y.is_finite()
        || !min_z.is_finite()
        || !max_z.is_finite()
    {
        return Err(AppError::validation(
            "tagged edge anchor resolution requires finite edge endpoints.".to_string(),
        ));
    }
    let span = (max_x - min_x)
        .abs()
        .max((max_y - min_y).abs())
        .max((max_z - min_z).abs())
        .max(1.0);
    Ok(EdgeTargetExtrema {
        min_x,
        max_x,
        min_y,
        max_y,
        min_z,
        max_z,
        tol: coordinate_tolerance(span),
    })
}

fn face_target_bounds(face_targets: &[&ViewerFaceTarget]) -> AppResult<FaceTargetBounds> {
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut min_z = f64::INFINITY;
    let mut max_z = f64::NEG_INFINITY;
    for target in face_targets {
        min_x = min_x.min(target.center.x);
        max_x = max_x.max(target.center.x);
        min_y = min_y.min(target.center.y);
        max_y = max_y.max(target.center.y);
        min_z = min_z.min(target.center.z);
        max_z = max_z.max(target.center.z);
    }
    if !min_x.is_finite()
        || !max_x.is_finite()
        || !min_y.is_finite()
        || !max_y.is_finite()
        || !min_z.is_finite()
        || !max_z.is_finite()
    {
        return Err(AppError::validation(
            "tagged face anchor resolution requires finite face centers.".to_string(),
        ));
    }
    let span = (max_x - min_x)
        .abs()
        .max((max_y - min_y).abs())
        .max((max_z - min_z).abs())
        .max(1.0);
    Ok(FaceTargetBounds {
        min_x,
        max_x,
        min_y,
        max_y,
        min_z,
        max_z,
        span,
    })
}

fn coordinate_tolerance(span: f64) -> f64 {
    span.max(1.0) * 1e-6
}

fn edge_target_matches_clause(
    target: &ViewerEdgeTarget,
    clause: EdgeSelectorClause,
    extrema: EdgeTargetExtrema,
) -> bool {
    match clause {
        EdgeSelectorClause::Axis(axis) => edge_target_matches_axis(target, axis, extrema.tol),
        EdgeSelectorClause::Boundary { axis, bound } => {
            edge_target_matches_boundary(target, axis, bound, extrema)
        }
    }
}

fn edge_target_matches_axis(target: &ViewerEdgeTarget, axis: EdgeAxis, tol: f64) -> bool {
    let dx = (target.end.x - target.start.x).abs();
    let dy = (target.end.y - target.start.y).abs();
    let dz = (target.end.z - target.start.z).abs();
    match axis {
        EdgeAxis::X => dx > tol && dy <= tol && dz <= tol,
        EdgeAxis::Y => dy > tol && dx <= tol && dz <= tol,
        EdgeAxis::Z => dz > tol && dx <= tol && dy <= tol,
    }
}

fn edge_target_matches_boundary(
    target: &ViewerEdgeTarget,
    axis: EdgeAxis,
    bound: EdgeBound,
    extrema: EdgeTargetExtrema,
) -> bool {
    let (start, end, boundary) = match (axis, bound) {
        (EdgeAxis::X, EdgeBound::Min) => (target.start.x, target.end.x, extrema.min_x),
        (EdgeAxis::X, EdgeBound::Max) => (target.start.x, target.end.x, extrema.max_x),
        (EdgeAxis::Y, EdgeBound::Min) => (target.start.y, target.end.y, extrema.min_y),
        (EdgeAxis::Y, EdgeBound::Max) => (target.start.y, target.end.y, extrema.max_y),
        (EdgeAxis::Z, EdgeBound::Min) => (target.start.z, target.end.z, extrema.min_z),
        (EdgeAxis::Z, EdgeBound::Max) => (target.start.z, target.end.z, extrema.max_z),
    };
    (start - boundary).abs() <= extrema.tol && (end - boundary).abs() <= extrema.tol
}

fn filter_face_targets_by_area(
    face_targets: Vec<&ViewerFaceTarget>,
    rank: FaceAreaRank,
) -> Vec<&ViewerFaceTarget> {
    let areas = face_targets
        .iter()
        .filter_map(|target| target.area)
        .collect::<Vec<_>>();
    if areas.is_empty() {
        return Vec::new();
    }
    let target_area = match rank {
        FaceAreaRank::Min => areas.into_iter().fold(f64::INFINITY, f64::min),
        FaceAreaRank::Max => areas.into_iter().fold(f64::NEG_INFINITY, f64::max),
    };
    let tol = coordinate_tolerance(target_area.abs().max(1.0));
    face_targets
        .into_iter()
        .filter(|target| {
            target
                .area
                .map(|area| (area - target_area).abs() <= tol)
                .unwrap_or(false)
        })
        .collect()
}

fn face_target_matches_clause(
    target: &ViewerFaceTarget,
    clause: FaceSelectorClause,
    bounds: FaceTargetBounds,
    tol: f64,
) -> bool {
    match clause {
        FaceSelectorClause::Boundary { axis, bound } => {
            let coordinate = point_axis_value(&target.center, axis);
            let boundary = match (axis, bound) {
                (EdgeAxis::X, EdgeBound::Min) => bounds.min_x,
                (EdgeAxis::X, EdgeBound::Max) => bounds.max_x,
                (EdgeAxis::Y, EdgeBound::Min) => bounds.min_y,
                (EdgeAxis::Y, EdgeBound::Max) => bounds.max_y,
                (EdgeAxis::Z, EdgeBound::Min) => bounds.min_z,
                (EdgeAxis::Z, EdgeBound::Max) => bounds.max_z,
            };
            (coordinate - boundary).abs() <= tol
        }
        FaceSelectorClause::Planar => target.normal.is_some(),
        FaceSelectorClause::Normal(axis) => target
            .normal
            .map(|normal| normal_matches_axis(normal, axis))
            .unwrap_or(false),
        FaceSelectorClause::Area(_) => false,
    }
}

fn normal_matches_axis(normal: [f64; 3], axis: EdgeAxis) -> bool {
    let magnitude = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
    if magnitude <= f64::EPSILON {
        return false;
    }
    let unit = [
        normal[0] / magnitude,
        normal[1] / magnitude,
        normal[2] / magnitude,
    ];
    let dominant = match axis {
        EdgeAxis::X => unit[0].abs(),
        EdgeAxis::Y => unit[1].abs(),
        EdgeAxis::Z => unit[2].abs(),
    };
    let off_axis = match axis {
        EdgeAxis::X => unit[1].abs().max(unit[2].abs()),
        EdgeAxis::Y => unit[0].abs().max(unit[2].abs()),
        EdgeAxis::Z => unit[0].abs().max(unit[1].abs()),
    };
    dominant >= 1.0 - 1e-6 && off_axis <= 1e-6
}

fn point_axis_value(point: &ViewerEdgePoint, axis: EdgeAxis) -> f64 {
    match axis {
        EdgeAxis::X => point.x,
        EdgeAxis::Y => point.y,
        EdgeAxis::Z => point.z,
    }
}

fn topology_target_matches_selector_target(target_id: &str, selector_target: &str) -> bool {
    topology_target_part_key(target_id)
        .map(|part_key| part_key == selector_target)
        .unwrap_or(false)
}

fn topology_target_part_key(target_id: &str) -> Option<&str> {
    target_id
        .split_once(":edge:")
        .map(|(part_key, _)| part_key)
        .or_else(|| target_id.split_once(":face:").map(|(part_key, _)| part_key))
}

pub(crate) fn rebind_program_tagged_selectors(
    program: &CoreProgram,
    previous_manifest: Option<&ModelManifest>,
) -> AppResult<CoreProgram> {
    let mut rebound = program.clone();
    rebound.parts = program
        .parts
        .iter()
        .map(|part| {
            Ok(crate::ecky_core_ir::CorePart {
                root: rebind_node_tagged_selectors(
                    &part.root,
                    &program.selector_tags,
                    previous_manifest,
                )?,
                ..part.clone()
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    Ok(rebound)
}

pub(crate) fn preferred_public_topology_target_id(
    selection_target: &SelectionTarget,
    fallback_target_id: &str,
) -> String {
    selection_target
        .alias_ids
        .iter()
        .find(|alias_id| is_stable_topology_target_id(alias_id))
        .cloned()
        .or_else(|| selection_target.target_id.clone())
        .unwrap_or_else(|| fallback_target_id.to_string())
}

pub(crate) fn viewer_target_alias_ids(
    selection_target: &SelectionTarget,
    fallback_target_id: &str,
) -> Vec<String> {
    let public_target_id =
        preferred_public_topology_target_id(selection_target, fallback_target_id);
    let mut alias_ids = Vec::new();
    if fallback_target_id != public_target_id {
        alias_ids.push(fallback_target_id.to_string());
    }
    if let Some(target_id) = selection_target.target_id.as_ref() {
        if target_id != &public_target_id && !alias_ids.contains(target_id) {
            alias_ids.push(target_id.clone());
        }
    }
    if let Some(durable_target_id) = selection_target.durable_target_id.as_ref() {
        if durable_target_id != &public_target_id && !alias_ids.contains(durable_target_id) {
            alias_ids.push(durable_target_id.clone());
        }
    }
    if let Some(canonical_target_id) = selection_target.canonical_target_id.as_ref() {
        if canonical_target_id != &public_target_id && !alias_ids.contains(canonical_target_id) {
            alias_ids.push(canonical_target_id.clone());
        }
    }
    for alias_id in &selection_target.alias_ids {
        if alias_id != &public_target_id && !alias_ids.contains(alias_id) {
            alias_ids.push(alias_id.clone());
        }
    }
    alias_ids
}

pub(crate) fn is_stable_topology_target_id(target_id: &str) -> bool {
    [":edge:", ":face:"].into_iter().any(|marker| {
        let Some((_, payload)) = target_id.split_once(marker) else {
            return false;
        };
        let Some(first) = payload.split(':').next() else {
            return false;
        };
        !first.chars().all(|ch| ch.is_ascii_digit())
    })
}

pub(crate) fn stable_edge_target_id(target_id: &str) -> String {
    stable_topology_target_id(target_id, ":edge:", 2)
}

pub(crate) fn stable_face_target_id(target_id: &str) -> String {
    stable_topology_target_id(target_id, ":face:", 3)
}

pub(crate) fn durable_edge_target_id(
    part_id: &str,
    root_node_id: u64,
    target_id: &str,
) -> Option<String> {
    durable_topology_target_id(part_id, root_node_id, target_id, ":edge:")
}

pub(crate) fn durable_edge_target_id_for_stable_node_key(
    part_id: &str,
    stable_node_key: &str,
    target_id: &str,
) -> Option<String> {
    durable_topology_target_id_for_stable_node_key(part_id, stable_node_key, target_id, ":edge:")
}

pub(crate) fn durable_face_target_id(
    part_id: &str,
    root_node_id: u64,
    target_id: &str,
) -> Option<String> {
    durable_topology_target_id(part_id, root_node_id, target_id, ":face:")
}

pub(crate) fn durable_face_target_id_for_stable_node_key(
    part_id: &str,
    stable_node_key: &str,
    target_id: &str,
) -> Option<String> {
    durable_topology_target_id_for_stable_node_key(part_id, stable_node_key, target_id, ":face:")
}

pub(crate) fn topology_target_aliases(
    _public_target_id: &str,
    _canonical_target_id: String,
) -> Vec<String> {
    Vec::new()
}

pub(crate) fn portable_topology_target_id(target_id: &str) -> Option<String> {
    portable_topology_target_id_with_marker(target_id, ":edge:")
        .or_else(|| portable_topology_target_id_with_marker(target_id, ":face:"))
}

fn stable_topology_target_id(target_id: &str, marker: &str, minimum_parts: usize) -> String {
    let raw = target_id.trim();
    let Some((prefix, payload)) = raw.split_once(marker) else {
        return raw.to_string();
    };
    let parts = payload.split(':').collect::<Vec<_>>();
    if parts.len() >= minimum_parts && parts[0].chars().all(|ch| ch.is_ascii_digit()) {
        return format!("{prefix}{marker}{}", parts[1..].join(":"));
    }
    raw.to_string()
}

fn durable_topology_target_id(
    part_id: &str,
    root_node_id: u64,
    target_id: &str,
    marker: &str,
) -> Option<String> {
    let (_, payload) = target_id.trim().split_once(marker)?;
    Some(format!("{part_id}:node:{root_node_id}{marker}{payload}"))
}

fn durable_topology_target_id_for_stable_node_key(
    part_id: &str,
    stable_node_key: &str,
    target_id: &str,
    marker: &str,
) -> Option<String> {
    let stable_node_key = stable_node_key.trim();
    if stable_node_key.is_empty() {
        return None;
    }
    let (_, payload) = target_id.trim().split_once(marker)?;
    Some(format!(
        "{part_id}:stable-node-key:{stable_node_key}{marker}{payload}"
    ))
}

fn portable_topology_target_id_with_marker(target_id: &str, marker: &str) -> Option<String> {
    let (_, payload) = target_id.split_once(marker)?;
    let parts = payload.split(':').collect::<Vec<_>>();
    if parts.is_empty() {
        return None;
    }
    let normalized_payload = if parts[0].chars().all(|ch| ch.is_ascii_digit()) {
        parts.get(1..)?.join(":")
    } else {
        payload.to_string()
    };
    if normalized_payload.is_empty() {
        return None;
    }
    Some(format!(
        "{}{}",
        &marker[1..],
        normalize_portable_topology_payload(marker, &normalized_payload)
            .unwrap_or(normalized_payload)
    ))
}

fn normalize_portable_topology_payload(marker: &str, payload: &str) -> Option<String> {
    match marker {
        ":edge:" => {
            let (start, end) = payload.split_once('_')?;
            let start = parse_point_signature(start)?;
            let end = parse_point_signature(end)?;
            Some(format!(
                "{}_{}",
                format_point_signature(&start),
                format_point_signature(&end)
            ))
        }
        ":face:" => {
            let (center, area) = payload.split_once(':')?;
            let center = parse_point_signature(center)?;
            let area = area.parse::<f64>().ok()?;
            Some(format!(
                "{}:{}",
                format_point_signature(&center),
                format_portable_topology_number(area)
            ))
        }
        _ => None,
    }
}

fn parse_point_signature(signature: &str) -> Option<[f64; 3]> {
    let mut values = Vec::new();
    let mut negative = false;
    for part in signature.split('-') {
        if part.is_empty() {
            negative = true;
            continue;
        }
        let mut value = part.parse::<f64>().ok()?;
        if negative {
            value = -value;
            negative = false;
        }
        values.push(value);
    }
    if negative || values.len() != 3 {
        return None;
    }
    Some([values[0], values[1], values[2]])
}

fn format_point_signature(point: &[f64; 3]) -> String {
    point
        .iter()
        .map(|value| format_portable_topology_number(*value))
        .collect::<Vec<_>>()
        .join("-")
}

fn selector_tag_selection_kind(kind: CoreSelectorTagKind) -> SelectionTargetKind {
    match kind {
        CoreSelectorTagKind::Face => SelectionTargetKind::Face,
        CoreSelectorTagKind::Edge => SelectionTargetKind::Edge,
    }
}

fn rebind_node_tagged_selectors(
    node: &CoreNode,
    selector_tags: &[CoreSelectorTagDecl],
    previous_manifest: Option<&ModelManifest>,
) -> AppResult<CoreNode> {
    let kind = match &node.kind {
        CoreNodeKind::Literal(_) | CoreNodeKind::Reference(_) => node.kind.clone(),
        CoreNodeKind::Build { bindings, result } => CoreNodeKind::Build {
            bindings: bindings
                .iter()
                .map(|binding| {
                    Ok(crate::ecky_core_ir::CoreShapeBinding {
                        value: rebind_node_tagged_selectors(
                            &binding.value,
                            selector_tags,
                            previous_manifest,
                        )?,
                        ..binding.clone()
                    })
                })
                .collect::<AppResult<Vec<_>>>()?,
            result: Box::new(rebind_node_tagged_selectors(
                result,
                selector_tags,
                previous_manifest,
            )?),
        },
        CoreNodeKind::Let { bindings, body } => CoreNodeKind::Let {
            bindings: bindings
                .iter()
                .map(|binding| {
                    Ok(crate::ecky_core_ir::CoreBinding {
                        value: rebind_node_tagged_selectors(
                            &binding.value,
                            selector_tags,
                            previous_manifest,
                        )?,
                        ..binding.clone()
                    })
                })
                .collect::<AppResult<Vec<_>>>()?,
            body: Box::new(rebind_node_tagged_selectors(
                body,
                selector_tags,
                previous_manifest,
            )?),
        },
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => CoreNodeKind::If {
            condition: Box::new(rebind_node_tagged_selectors(
                condition,
                selector_tags,
                previous_manifest,
            )?),
            then_branch: Box::new(rebind_node_tagged_selectors(
                then_branch,
                selector_tags,
                previous_manifest,
            )?),
            else_branch: Box::new(rebind_node_tagged_selectors(
                else_branch,
                selector_tags,
                previous_manifest,
            )?),
        },
        CoreNodeKind::Call { op, args, keywords } => CoreNodeKind::Call {
            op: op.clone(),
            args: args
                .iter()
                .map(|arg| rebind_node_tagged_selectors(arg, selector_tags, previous_manifest))
                .collect::<AppResult<Vec<_>>>()?,
            keywords: keywords
                .iter()
                .map(|keyword| {
                    rebind_keyword_tagged_selectors(keyword, selector_tags, previous_manifest)
                })
                .collect::<AppResult<Vec<_>>>()?,
        },
        CoreNodeKind::Range { start, end } => CoreNodeKind::Range {
            start: Box::new(rebind_node_tagged_selectors(
                start,
                selector_tags,
                previous_manifest,
            )?),
            end: Box::new(rebind_node_tagged_selectors(
                end,
                selector_tags,
                previous_manifest,
            )?),
        },
        CoreNodeKind::Map {
            params,
            sources,
            body,
        } => CoreNodeKind::Map {
            params: params.clone(),
            sources: sources
                .iter()
                .map(|source| {
                    rebind_node_tagged_selectors(source, selector_tags, previous_manifest)
                })
                .collect::<AppResult<Vec<_>>>()?,
            body: Box::new(rebind_node_tagged_selectors(
                body,
                selector_tags,
                previous_manifest,
            )?),
        },
        CoreNodeKind::Apply { op, args, list } => CoreNodeKind::Apply {
            op: op.clone(),
            args: args
                .iter()
                .map(|arg| rebind_node_tagged_selectors(arg, selector_tags, previous_manifest))
                .collect::<AppResult<Vec<_>>>()?,
            list: Box::new(rebind_node_tagged_selectors(
                list,
                selector_tags,
                previous_manifest,
            )?),
        },
        CoreNodeKind::List(items) => CoreNodeKind::List(
            items
                .iter()
                .map(|item| rebind_node_tagged_selectors(item, selector_tags, previous_manifest))
                .collect::<AppResult<Vec<_>>>()?,
        ),
        CoreNodeKind::Group(items) => CoreNodeKind::Group(
            items
                .iter()
                .map(|item| rebind_node_tagged_selectors(item, selector_tags, previous_manifest))
                .collect::<AppResult<Vec<_>>>()?,
        ),
    };

    Ok(CoreNode {
        kind,
        ..node.clone()
    })
}

fn rebind_keyword_tagged_selectors(
    keyword: &CoreKeywordArg,
    selector_tags: &[CoreSelectorTagDecl],
    previous_manifest: Option<&ModelManifest>,
) -> AppResult<CoreKeywordArg> {
    let value = match &keyword.value {
        CoreKeywordValue::Expr(node) => CoreKeywordValue::Expr(rebind_node_tagged_selectors(
            node,
            selector_tags,
            previous_manifest,
        )?),
        CoreKeywordValue::Selector { source, payload } => CoreKeywordValue::Selector {
            source: rebind_node_tagged_selectors(source, selector_tags, previous_manifest)?,
            payload: rebind_selector_payload_tagged_targets(
                payload,
                selector_tags,
                previous_manifest,
            )?,
        },
    };
    Ok(CoreKeywordArg {
        value,
        ..keyword.clone()
    })
}

fn rebind_selector_payload_tagged_targets(
    payload: &CoreSelectorPayload,
    selector_tags: &[CoreSelectorTagDecl],
    previous_manifest: Option<&ModelManifest>,
) -> AppResult<CoreSelectorPayload> {
    match payload {
        CoreSelectorPayload::EdgeTag(tag_name) => resolve_tagged_selector_payload(
            tag_name,
            CoreSelectorTagKind::Edge,
            selector_tags,
            previous_manifest,
        ),
        CoreSelectorPayload::FaceTag(tag_name) => resolve_tagged_selector_payload(
            tag_name,
            CoreSelectorTagKind::Face,
            selector_tags,
            previous_manifest,
        ),
        CoreSelectorPayload::EdgeTargetIds(target_ids) => {
            if let Some(tag_name) = tagged_selector_placeholder_name(target_ids) {
                return resolve_tagged_selector_payload(
                    tag_name,
                    CoreSelectorTagKind::Edge,
                    selector_tags,
                    previous_manifest,
                );
            }
            Ok(payload.clone())
        }
        CoreSelectorPayload::FaceTargetIds(target_ids) => {
            if let Some(tag_name) = tagged_selector_placeholder_name(target_ids) {
                return resolve_tagged_selector_payload(
                    tag_name,
                    CoreSelectorTagKind::Face,
                    selector_tags,
                    previous_manifest,
                );
            }
            Ok(payload.clone())
        }
        _ => Ok(payload.clone()),
    }
}

fn tagged_selector_placeholder_name(target_ids: &[String]) -> Option<&str> {
    if target_ids.len() != 1 {
        return None;
    }
    target_ids[0].strip_prefix("tag:")
}

fn resolve_tagged_selector_payload(
    tag_name: &str,
    expected_kind: CoreSelectorTagKind,
    selector_tags: &[CoreSelectorTagDecl],
    previous_manifest: Option<&ModelManifest>,
) -> AppResult<CoreSelectorPayload> {
    if let Some(previous_manifest) = previous_manifest {
        if let Some(anchor) = previous_manifest.tagged_anchors.get(tag_name) {
            let anchor_kind = match anchor.kind {
                TaggedAnchorKind::Face => CoreSelectorTagKind::Face,
                TaggedAnchorKind::Edge => CoreSelectorTagKind::Edge,
            };
            if anchor_kind != expected_kind {
                return Err(AppError::validation(format!(
                    "Tagged selector `(tag {tag_name})` expected {} ids, but previous manifest recorded {} anchor ids.",
                    selector_tag_kind_label(expected_kind),
                    selector_tag_kind_label(anchor_kind)
                )));
            }
            if !anchor.target_ids.is_empty() {
                return Ok(match anchor_kind {
                    CoreSelectorTagKind::Face => {
                        CoreSelectorPayload::FaceTargetIds(anchor.target_ids.clone())
                    }
                    CoreSelectorTagKind::Edge => {
                        CoreSelectorPayload::EdgeTargetIds(anchor.target_ids.clone())
                    }
                });
            }
            return Err(AppError::validation(format!(
                "Tagged selector `(tag {tag_name})` has no recorded target ids in previous manifest.",
            )));
        }
    }

    let selector_tag = selector_tags
        .iter()
        .find(|selector_tag| selector_tag.name == tag_name)
        .ok_or_else(|| {
            AppError::validation(format!(
                "Tagged selector `(tag {tag_name})` has no matching {} tag declaration.",
                selector_tag_kind_label(expected_kind)
            ))
        })?;
    if selector_tag.kind != expected_kind {
        return Err(AppError::validation(format!(
            "Tagged selector `(tag {tag_name})` expected {} ids, but declaration is {}.",
            selector_tag_kind_label(expected_kind),
            selector_tag_kind_label(selector_tag.kind)
        )));
    }

    let rebound_payload = match selector_tag.kind {
        CoreSelectorTagKind::Face => {
            parse_core_face_selector_payload(&selector_tag.authored_selector)?
        }
        CoreSelectorTagKind::Edge => {
            parse_core_edge_selector_payload(&selector_tag.authored_selector)?
        }
    };
    let resolved_exact_target_ids = match &rebound_payload {
        CoreSelectorPayload::FaceTargetIds(target_ids)
        | CoreSelectorPayload::EdgeTargetIds(target_ids) => Some(target_ids),
        _ => None,
    };
    if resolved_exact_target_ids.is_some_and(|target_ids| target_ids.is_empty()) {
        return Err(AppError::validation(format!(
            "Tagged selector `(tag {tag_name})` resolved no exact target ids.",
        )));
    }
    Ok(rebound_payload)
}

fn selector_tag_kind_label(kind: CoreSelectorTagKind) -> &'static str {
    match kind {
        CoreSelectorTagKind::Face => "face",
        CoreSelectorTagKind::Edge => "edge",
    }
}

fn selection_target_matches_id(
    selection_target: &SelectionTarget,
    requested_target_id: &str,
) -> bool {
    selection_target.target_id.as_deref() == Some(requested_target_id)
        || selection_target.durable_target_id.as_deref() == Some(requested_target_id)
        || selection_target.canonical_target_id.as_deref() == Some(requested_target_id)
        || selection_target
            .alias_ids
            .iter()
            .any(|alias_id| alias_id == requested_target_id)
}

fn push_unique_option(target: &mut Vec<String>, value: Option<&String>) {
    if let Some(value) = value {
        if !target.iter().any(|existing| existing == value) {
            target.push(value.clone());
        }
    }
}

fn push_unique_str_option(target: &mut Vec<String>, value: Option<&str>) {
    if let Some(value) = value {
        push_unique_value(target, value);
    }
}

fn push_unique_value(target: &mut Vec<String>, value: &str) {
    if !target.iter().any(|existing| existing == value) {
        target.push(value.to_string());
    }
}

fn push_unique_strings(target: &mut Vec<String>, values: &[String]) {
    for value in values {
        if !target.iter().any(|existing| existing == value) {
            target.push(value.clone());
        }
    }
}

fn push_unique_str_slice(target: &mut Vec<String>, values: &[String]) {
    for value in values {
        push_unique_value(target, value);
    }
}

fn format_portable_topology_number(value: f64) -> String {
    let rounded = (value * 1000.0).round() / 1000.0;
    if rounded.abs() < 0.0005 {
        return "0".to_string();
    }
    let mut text = format!("{rounded:.3}");
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::contracts::{ViewerEdgePoint, ViewerEdgeTarget, ViewerFaceTarget};
    use crate::ecky_core_ir::{
        CoreKeywordValue, CoreNodeKind, CoreSelectorPayload, CoreSelectorTagDecl,
        CoreSelectorTagKind,
    };
    use crate::ecky_scheme::compile_to_core_program;
    use crate::models::{
        DocumentMetadata, EngineKind, EnrichmentStatus, GeometryBackend, ManifestEnrichmentState,
        ModelManifest, ModelSourceKind, SourceLanguage, TaggedAnchorBinding, TaggedAnchorKind,
    };
    use crate::models::{SelectionTarget, SelectionTargetKind};

    use super::{
        durable_edge_target_id, durable_edge_target_id_for_stable_node_key, durable_face_target_id,
        durable_face_target_id_for_stable_node_key, is_stable_topology_target_id,
        portable_topology_target_id, preferred_public_topology_target_id,
        rebind_program_tagged_selectors, resolve_tagged_anchors, stable_edge_target_id,
        stable_face_target_id, topology_target_aliases, viewer_target_alias_ids,
    };

    fn selection_target(target_id: &str, alias_ids: &[&str]) -> SelectionTarget {
        SelectionTarget {
            target_id: Some(target_id.to_string()),
            durable_target_id: None,
            canonical_target_id: None,
            alias_ids: alias_ids.iter().map(|value| value.to_string()).collect(),
            part_id: "body".to_string(),
            viewer_node_id: "node".to_string(),
            label: "Edge 1".to_string(),
            kind: SelectionTargetKind::Edge,
            editable: true,
            parameter_keys: Vec::new(),
            primitive_ids: Vec::new(),
            view_ids: Vec::new(),
        }
    }

    fn face_selection_target(
        target_id: &str,
        durable_target_id: Option<&str>,
        canonical_target_id: Option<&str>,
        alias_ids: &[&str],
    ) -> SelectionTarget {
        SelectionTarget {
            target_id: Some(target_id.to_string()),
            durable_target_id: durable_target_id.map(str::to_string),
            canonical_target_id: canonical_target_id.map(str::to_string),
            alias_ids: alias_ids.iter().map(|value| value.to_string()).collect(),
            part_id: "body".to_string(),
            viewer_node_id: "node".to_string(),
            label: "Face".to_string(),
            kind: SelectionTargetKind::Face,
            editable: true,
            parameter_keys: Vec::new(),
            primitive_ids: Vec::new(),
            view_ids: Vec::new(),
        }
    }

    fn face_target(
        target_id: &str,
        durable_target_id: Option<&str>,
        canonical_target_id: Option<&str>,
        alias_ids: &[&str],
        z: f64,
    ) -> ViewerFaceTarget {
        ViewerFaceTarget {
            target_id: target_id.to_string(),
            durable_target_id: durable_target_id.map(str::to_string),
            canonical_target_id: canonical_target_id.map(str::to_string),
            alias_ids: alias_ids.iter().map(|value| value.to_string()).collect(),
            part_id: "body".to_string(),
            viewer_node_id: "node".to_string(),
            label: "Face".to_string(),
            editable: true,
            center: ViewerEdgePoint { x: 0.0, y: 0.0, z },
            normal: Some([0.0, 0.0, 1.0]),
            area: Some(100.0),
        }
    }

    fn edge_target(
        target_id: &str,
        start: ViewerEdgePoint,
        end: ViewerEdgePoint,
    ) -> ViewerEdgeTarget {
        ViewerEdgeTarget {
            target_id: target_id.to_string(),
            durable_target_id: None,
            canonical_target_id: None,
            alias_ids: Vec::new(),
            part_id: "body".to_string(),
            viewer_node_id: "node".to_string(),
            label: "Edge".to_string(),
            editable: true,
            start,
            end,
        }
    }

    #[test]
    fn stable_topology_target_ids_drop_numeric_indexes_only() {
        assert_eq!(
            stable_edge_target_id("body:edge:0:0-0-0_10-0-0"),
            "body:edge:0-0-0_10-0-0"
        );
        assert_eq!(
            stable_face_target_id("body:face:5:0-0-10:100"),
            "body:face:0-0-10:100"
        );
        assert_eq!(
            stable_edge_target_id("body:edge:0-0-0_10-0-0"),
            "body:edge:0-0-0_10-0-0"
        );
    }

    #[test]
    fn viewer_aliases_prefer_stable_public_id() {
        let target = selection_target(
            "body:edge:0:0-0-0_10-0-0",
            &["body:edge:0-0-0_10-0-0", "legacy-edge"],
        );
        assert_eq!(
            preferred_public_topology_target_id(&target, "fallback-edge"),
            "body:edge:0-0-0_10-0-0"
        );
        assert_eq!(
            viewer_target_alias_ids(&target, "fallback-edge"),
            vec![
                "fallback-edge".to_string(),
                "body:edge:0:0-0-0_10-0-0".to_string(),
                "legacy-edge".to_string()
            ]
        );
    }

    #[test]
    fn stable_topology_detection_and_alias_emission_match() {
        assert!(is_stable_topology_target_id("body:face:0-0-10:100"));
        assert!(!is_stable_topology_target_id("body:face:5:0-0-10:100"));
        assert!(topology_target_aliases(
            "body:edge:0-0-0_10-0-0",
            "body:edge:0:0-0-0_10-0-0".into()
        )
        .is_empty());
    }

    #[test]
    fn portable_topology_target_ids_normalize_precision_and_indexes() {
        assert_eq!(
            portable_topology_target_id("body:edge:0:0.0002--0.0002-0_10.0004-0-0"),
            Some("edge:0-0-0_10-0-0".to_string())
        );
        assert_eq!(
            portable_topology_target_id("body:face:5:0.0001-0-10.0002:100.0004"),
            Some("face:0-0-10:100".to_string())
        );
    }

    #[test]
    fn durable_topology_target_ids_prefix_root_node_id() {
        assert_eq!(
            durable_edge_target_id("body", 42, "body:edge:0-0-0_10-0-0").as_deref(),
            Some("body:node:42:edge:0-0-0_10-0-0")
        );
        assert_eq!(
            durable_face_target_id("body", 42, "body:face:0-0-10:100").as_deref(),
            Some("body:node:42:face:0-0-10:100")
        );
    }

    #[test]
    fn durable_topology_target_ids_accept_stable_node_key() {
        assert_eq!(
            durable_edge_target_id_for_stable_node_key(
                "body",
                "sha256:abcdef",
                "body:edge:0-0-0_10-0-0",
            )
            .as_deref(),
            Some("body:stable-node-key:sha256:abcdef:edge:0-0-0_10-0-0")
        );
        assert_eq!(
            durable_face_target_id_for_stable_node_key(
                "body",
                "sha256:abcdef",
                "body:face:0-0-10:100",
            )
            .as_deref(),
            Some("body:stable-node-key:sha256:abcdef:face:0-0-10:100")
        );
    }

    #[test]
    fn resolve_tagged_anchors_records_clause_face_targets() {
        let selector_tags = vec![CoreSelectorTagDecl {
            name: "mounting_top".to_string(),
            kind: CoreSelectorTagKind::Face,
            authored_selector: "top".to_string(),
            target: "body".to_string(),
        }];
        let selection_targets = vec![
            face_selection_target(
                "body:face:0-0-10:100",
                Some("body:node:42:face:0-0-10:100"),
                Some("body:face:5:0-0-10:100"),
                &["body:face:legacy-top"],
            ),
            face_selection_target(
                "body:face:0-0-0:100",
                Some("body:node:42:face:0-0-0:100"),
                Some("body:face:2:0-0-0:100"),
                &[],
            ),
        ];
        let face_targets = vec![
            face_target(
                "body:face:0-0-10:100",
                Some("body:node:42:face:0-0-10:100"),
                Some("body:face:5:0-0-10:100"),
                &["body:face:legacy-top"],
                10.0,
            ),
            face_target(
                "body:face:0-0-0:100",
                Some("body:node:42:face:0-0-0:100"),
                Some("body:face:2:0-0-0:100"),
                &[],
                0.0,
            ),
        ];

        let anchors =
            resolve_tagged_anchors(&selector_tags, &selection_targets, &[], &face_targets)
                .expect("anchors");
        let anchor = anchors.get("mounting_top").expect("mounting_top");
        assert_eq!(anchor.kind, TaggedAnchorKind::Face);
        assert_eq!(anchor.authored_selector, "top");
        assert_eq!(anchor.target, "body");
        assert_eq!(anchor.target_ids, vec!["body:face:0-0-10:100".to_string()]);
        assert_eq!(
            anchor.durable_target_ids,
            vec!["body:node:42:face:0-0-10:100".to_string()]
        );
        assert_eq!(
            anchor.canonical_target_ids,
            vec!["body:face:5:0-0-10:100".to_string()]
        );
        assert_eq!(anchor.alias_ids, vec!["body:face:legacy-top".to_string()]);
    }

    #[test]
    fn resolve_tagged_anchors_records_clause_edge_targets() {
        let selector_tags = vec![CoreSelectorTagDecl {
            name: "vertical_edge".to_string(),
            kind: CoreSelectorTagKind::Edge,
            authored_selector: "vertical".to_string(),
            target: "body".to_string(),
        }];
        let edge_targets = vec![
            edge_target(
                "body:edge:0-0-0_0-0-10",
                ViewerEdgePoint {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                ViewerEdgePoint {
                    x: 0.0,
                    y: 0.0,
                    z: 10.0,
                },
            ),
            edge_target(
                "body:edge:0-0-0_10-0-0",
                ViewerEdgePoint {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                ViewerEdgePoint {
                    x: 10.0,
                    y: 0.0,
                    z: 0.0,
                },
            ),
        ];

        let anchors =
            resolve_tagged_anchors(&selector_tags, &[], &edge_targets, &[]).expect("anchors");
        let anchor = anchors.get("vertical_edge").expect("vertical_edge");
        assert_eq!(
            anchor.target_ids,
            vec!["body:edge:0-0-0_0-0-10".to_string()]
        );
    }

    #[test]
    fn tagged_selector_rebind_prefers_previous_manifest_anchor_ids() {
        let program = compile_to_core_program(
            r#"
            (model
              (tag-face mounting_top :faces "top" body)
              (part body
                (shell 1 (box 10 10 10) :faces (tag mounting_top))))
            "#,
        )
        .expect("program");

        let rebound =
            rebind_program_tagged_selectors(&program, Some(&sample_manifest())).expect("rebound");

        let CoreNodeKind::Call { keywords, .. } = &rebound.parts[0].root.kind else {
            panic!("expected call root");
        };
        let payload = keywords
            .iter()
            .find_map(|keyword| match &keyword.value {
                CoreKeywordValue::Selector { payload, .. } => Some(payload),
                _ => None,
            })
            .expect("selector payload");
        assert_eq!(
            payload,
            &CoreSelectorPayload::FaceTargetIds(vec!["body:face:0-0-10:100".to_string()])
        );
    }

    #[test]
    fn tagged_selector_rebind_falls_back_to_authored_selector_without_previous_manifest() {
        let program = compile_to_core_program(
            r#"
            (model
              (tag-face mounting_top :faces "top" body)
              (part body
                (shell 1 (box 10 10 10) :faces (tag mounting_top))))
            "#,
        )
        .expect("program");

        let rebound = rebind_program_tagged_selectors(&program, None).expect("rebound");
        let CoreNodeKind::Call { keywords, .. } = &rebound.parts[0].root.kind else {
            panic!("expected call root");
        };
        let payload = keywords
            .iter()
            .find_map(|keyword| match &keyword.value {
                CoreKeywordValue::Selector { payload, .. } => Some(payload),
                _ => None,
            })
            .expect("selector payload");
        assert_eq!(
            payload,
            &CoreSelectorPayload::FaceClauses(vec![
                crate::ecky_core_ir::CoreFaceSelectorClause::Boundary {
                    axis: crate::ecky_core_ir::CoreEdgeAxis::Z,
                    bound: crate::ecky_core_ir::CoreEdgeBound::Max,
                }
            ])
        );
    }

    fn sample_manifest() -> ModelManifest {
        ModelManifest {
            schema_version: 1,
            model_id: "model-1".to_string(),
            source_kind: ModelSourceKind::Generated,
            source_digest: None,
            core_digest: None,
            ast_schema_version: None,
            engine_kind: EngineKind::EckyIrV0,
            source_language: SourceLanguage::EckyIrV0,
            geometry_backend: GeometryBackend::EckyRust,
            document: DocumentMetadata {
                document_name: "doc".to_string(),
                document_label: "doc".to_string(),
                source_path: None,
                object_count: 1,
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
            tagged_anchors: BTreeMap::from([(
                "mounting_top".to_string(),
                TaggedAnchorBinding {
                    kind: TaggedAnchorKind::Face,
                    authored_selector: "top".to_string(),
                    target: "body".to_string(),
                    target_ids: vec!["body:face:0-0-10:100".to_string()],
                    durable_target_ids: Vec::new(),
                    canonical_target_ids: vec!["body:face:5:0-0-10:100".to_string()],
                    alias_ids: Vec::new(),
                },
            )]),
            feature_graph: None,
            correspondence_graph: None,
            warnings: Vec::new(),
            enrichment_state: ManifestEnrichmentState {
                status: EnrichmentStatus::None,
                proposals: Vec::new(),
            },
        }
    }
}
