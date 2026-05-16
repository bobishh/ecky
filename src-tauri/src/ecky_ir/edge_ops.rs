use std::collections::{BTreeMap, HashMap};

use csgrs::float_types::parry3d::na::{Point3, Vector3};
use csgrs::mesh::polygon::Polygon as IrPolygon;
use csgrs::mesh::vertex::Vertex as IrVertex;

use crate::ecky_core_ir::{
    CoreEdgeAxis, CoreEdgeBound, CoreEdgeSelectorClause, CoreFaceAreaRank, CoreFaceSelectorClause,
    CoreSelectorPayload,
};
use crate::models::{AppResult, ParamValue};

use super::eval_scalar::eval_stringish;
use super::model::{expr_keyword_name, IrExpr};
use super::shared::{validation, IrMesh};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EdgeAxis {
    X,
    Y,
    Z,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EdgeBound {
    Min,
    Max,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EdgeSelectorClause {
    Axis(EdgeAxis),
    Boundary { axis: EdgeAxis, bound: EdgeBound },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EdgeSelector {
    All,
    Top,
    Bottom,
    Vertical,
    Left,
    Right,
    Front,
    Back,
    PlaneMin(EdgeAxis),
    PlaneMax(EdgeAxis),
    Axis(EdgeAxis),
    Compound(Vec<EdgeSelectorClause>),
    TargetIds(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FaceSelector {
    Clauses(Vec<FaceSelectorClause>),
    TargetIds(Vec<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FaceAreaRank {
    Min,
    Max,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FaceSelectorClause {
    Boundary { axis: EdgeAxis, bound: EdgeBound },
    Planar,
    Normal(EdgeAxis),
    Area(FaceAreaRank),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EdgeSelectorSpec {
    canonical_string: String,
    python_payload_literal: String,
    target_ids: Option<Vec<String>>,
}

impl EdgeSelectorSpec {
    #[cfg(test)]
    pub(crate) fn canonical_string(&self) -> &str {
        &self.canonical_string
    }

    pub(crate) fn target_ids(&self) -> Option<&[String]> {
        self.target_ids.as_deref()
    }

    pub(crate) fn python_payload_literal(&self) -> &str {
        &self.python_payload_literal
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FaceSelectorSpec {
    canonical_string: String,
    python_payload_literal: String,
    target_ids: Vec<String>,
}

impl FaceSelectorSpec {
    #[cfg(test)]
    pub(crate) fn canonical_string(&self) -> &str {
        &self.canonical_string
    }

    #[cfg(test)]
    pub(crate) fn target_ids(&self) -> &[String] {
        self.target_ids.as_slice()
    }

    pub(crate) fn python_payload_literal(&self) -> &str {
        &self.python_payload_literal
    }
}

#[derive(Debug, Clone, Copy)]
struct EdgeExtrema {
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
    min_z: f64,
    max_z: f64,
    tol: f64,
}

/// A feature edge between two triangles, identified by canonical vertex indices.
#[derive(Debug, Clone)]
pub(super) struct FeatureEdge {
    /// Canonical vertex indices (a < b).
    vi_a: usize,
    vi_b: usize,
    /// Positions of the two endpoints.
    pos_a: Point3<f64>,
    pos_b: Point3<f64>,
    /// Normals of the two adjacent faces.
    normal_left: Vector3<f64>,
    normal_right: Vector3<f64>,
    /// Dihedral angle in radians (0 = coplanar, π = folded back).
    /// Used by fillet (Phase 2) to scale the arc profile.
    #[allow(dead_code)]
    dihedral: f64,
}

pub(super) const FEATURE_EDGE_DIHEDRAL_THRESHOLD: f64 = 0.35; // ~20 degrees

const EDGE_SELECTOR_HELP: &str =
    "`all`, `top`, `bottom`, `left`, `right`, `front`, `back`, `vertical`, `axis-x`, `axis-y`, `axis-z`, `x-min`, `x-max`, `y-min`, `y-max`, `z-min`, `z-max`, `target-id:<id>`, `target-ids:<id>|<id>`, or `+` intersections such as `x-min+axis-z`.";
const FACE_SELECTOR_HELP: &str =
    "`all`, `planar`, `normal-x`, `normal-y`, `normal-z`, `area-min`, `area-max`, `top`, `bottom`, `left`, `right`, `front`, `back`, `x-min`, `x-max`, `y-min`, `y-max`, `z-min`, `z-max`, `target-id:<id>`, `target-ids:<id>|<id>`, or `+` intersections such as `planar+normal-z+z-max`.";

impl EdgeSelector {
    pub(crate) fn clauses(&self) -> Option<Vec<EdgeSelectorClause>> {
        match self {
            EdgeSelector::All => None,
            EdgeSelector::Top => Some(vec![EdgeSelectorClause::Boundary {
                axis: EdgeAxis::Z,
                bound: EdgeBound::Max,
            }]),
            EdgeSelector::Bottom => Some(vec![EdgeSelectorClause::Boundary {
                axis: EdgeAxis::Z,
                bound: EdgeBound::Min,
            }]),
            EdgeSelector::Vertical => Some(vec![EdgeSelectorClause::Axis(EdgeAxis::Z)]),
            EdgeSelector::Left => Some(vec![EdgeSelectorClause::Boundary {
                axis: EdgeAxis::X,
                bound: EdgeBound::Min,
            }]),
            EdgeSelector::Right => Some(vec![EdgeSelectorClause::Boundary {
                axis: EdgeAxis::X,
                bound: EdgeBound::Max,
            }]),
            EdgeSelector::Front => Some(vec![EdgeSelectorClause::Boundary {
                axis: EdgeAxis::Y,
                bound: EdgeBound::Max,
            }]),
            EdgeSelector::Back => Some(vec![EdgeSelectorClause::Boundary {
                axis: EdgeAxis::Y,
                bound: EdgeBound::Min,
            }]),
            EdgeSelector::PlaneMin(axis) => Some(vec![EdgeSelectorClause::Boundary {
                axis: *axis,
                bound: EdgeBound::Min,
            }]),
            EdgeSelector::PlaneMax(axis) => Some(vec![EdgeSelectorClause::Boundary {
                axis: *axis,
                bound: EdgeBound::Max,
            }]),
            EdgeSelector::Axis(axis) => Some(vec![EdgeSelectorClause::Axis(*axis)]),
            EdgeSelector::Compound(clauses) => Some(clauses.clone()),
            EdgeSelector::TargetIds(_) => None,
        }
    }

    pub(super) fn canonical_string(&self) -> String {
        if let Some(target_ids) = self.target_ids() {
            return format!("target-ids:{}", target_ids.join("|"));
        }
        let Some(clauses) = self.clauses() else {
            return "all".to_string();
        };
        clauses
            .iter()
            .map(|clause| match clause {
                EdgeSelectorClause::Axis(axis) => format!("axis-{}", edge_axis_name(*axis)),
                EdgeSelectorClause::Boundary { axis, bound } => {
                    format!("{}-{}", edge_axis_name(*axis), edge_bound_name(*bound))
                }
            })
            .collect::<Vec<_>>()
            .join("+")
    }

    pub(super) fn target_ids(&self) -> Option<&[String]> {
        match self {
            EdgeSelector::TargetIds(target_ids) => Some(target_ids.as_slice()),
            _ => None,
        }
    }
}

impl FaceSelector {
    pub(crate) fn canonical_string(&self) -> String {
        match self {
            FaceSelector::Clauses(clauses) => {
                if clauses.is_empty() {
                    "all".to_string()
                } else {
                    clauses
                        .iter()
                        .map(|clause| match clause {
                            FaceSelectorClause::Boundary { axis, bound } => {
                                format!("{}-{}", edge_axis_name(*axis), edge_bound_name(*bound))
                            }
                            FaceSelectorClause::Planar => "planar".to_string(),
                            FaceSelectorClause::Normal(axis) => {
                                format!("normal-{}", edge_axis_name(*axis))
                            }
                            FaceSelectorClause::Area(rank) => match rank {
                                FaceAreaRank::Min => "area-min".to_string(),
                                FaceAreaRank::Max => "area-max".to_string(),
                            },
                        })
                        .collect::<Vec<_>>()
                        .join("+")
                }
            }
            FaceSelector::TargetIds(target_ids) => format!("target-ids:{}", target_ids.join("|")),
        }
    }

    pub(crate) fn target_ids(&self) -> &[String] {
        match self {
            FaceSelector::Clauses(_) => &[],
            FaceSelector::TargetIds(target_ids) => target_ids.as_slice(),
        }
    }
}

pub(super) fn parse_edge_selector_value(selector_str: &str) -> AppResult<EdgeSelector> {
    if let Some(target_ids) = exact_edge_target_ids_from_selector_str(selector_str)? {
        return Ok(EdgeSelector::TargetIds(target_ids));
    }
    let normalized = selector_str.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Err(validation(format!(
            "Unknown edge selector `{}`. Use {}",
            selector_str, EDGE_SELECTOR_HELP
        )));
    }
    match normalized.as_str() {
        "all" => return Ok(EdgeSelector::All),
        "top" => return Ok(EdgeSelector::Top),
        "bottom" => return Ok(EdgeSelector::Bottom),
        "vertical" => return Ok(EdgeSelector::Vertical),
        "left" => return Ok(EdgeSelector::Left),
        "right" => return Ok(EdgeSelector::Right),
        "front" => return Ok(EdgeSelector::Front),
        "back" => return Ok(EdgeSelector::Back),
        _ => {}
    }

    let mut clauses = Vec::new();
    for raw_token in normalized.split('+') {
        let token = raw_token.trim();
        if token.is_empty() {
            return Err(validation(format!(
                "Unknown edge selector `{}`. Use {}",
                selector_str, EDGE_SELECTOR_HELP
            )));
        }
        if token == "all" {
            return Err(validation(format!(
                "Edge selector `{}` cannot combine `all` with other clauses.",
                selector_str
            )));
        }
        clauses.push(parse_edge_selector_clause(token, selector_str)?);
    }

    if clauses.is_empty() {
        Ok(EdgeSelector::All)
    } else if clauses.len() == 1 {
        match clauses[0] {
            EdgeSelectorClause::Axis(axis) => Ok(EdgeSelector::Axis(axis)),
            EdgeSelectorClause::Boundary {
                axis,
                bound: EdgeBound::Min,
            } => Ok(EdgeSelector::PlaneMin(axis)),
            EdgeSelectorClause::Boundary {
                axis,
                bound: EdgeBound::Max,
            } => Ok(EdgeSelector::PlaneMax(axis)),
        }
    } else {
        Ok(EdgeSelector::Compound(clauses))
    }
}

pub(crate) fn parse_face_selector_value(selector_str: &str) -> AppResult<FaceSelector> {
    if let Some(target_ids) = exact_face_target_ids_from_selector_str(selector_str)? {
        return Ok(FaceSelector::TargetIds(target_ids));
    }
    let normalized = selector_str.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Err(validation(format!(
            "Unknown face selector `{}`. Use {}",
            selector_str, FACE_SELECTOR_HELP
        )));
    }
    if normalized == "all" {
        return Ok(FaceSelector::Clauses(Vec::new()));
    }
    let mut clauses = Vec::new();
    for raw_token in normalized.split('+') {
        let token = raw_token.trim();
        if token.is_empty() {
            return Err(validation(format!(
                "Unknown face selector `{}`. Use {}",
                selector_str, FACE_SELECTOR_HELP
            )));
        }
        if token == "all" {
            return Err(validation(format!(
                "Face selector `{}` cannot combine `all` with other clauses.",
                selector_str
            )));
        }
        clauses.push(parse_face_selector_clause(token, selector_str)?);
    }
    Ok(FaceSelector::Clauses(clauses))
}

pub(crate) fn parse_edge_selector_spec(selector_str: &str) -> AppResult<EdgeSelectorSpec> {
    let parsed = parse_edge_selector_value(selector_str)?;
    Ok(EdgeSelectorSpec {
        canonical_string: parsed.canonical_string(),
        python_payload_literal: edge_selector_python_payload_literal(&parsed),
        target_ids: parsed.target_ids().map(|ids| ids.to_vec()),
    })
}

pub(crate) fn parse_core_edge_selector_payload(
    selector_str: &str,
) -> AppResult<CoreSelectorPayload> {
    let parsed = parse_edge_selector_value(selector_str)?;
    Ok(match parsed {
        EdgeSelector::All => CoreSelectorPayload::EdgeAll,
        EdgeSelector::TargetIds(target_ids) => CoreSelectorPayload::EdgeTargetIds(target_ids),
        selector => {
            let clauses = selector
                .clauses()
                .unwrap_or_default()
                .into_iter()
                .map(core_edge_selector_clause_from_edge_clause)
                .collect();
            CoreSelectorPayload::EdgeClauses(clauses)
        }
    })
}

pub(crate) fn parse_core_face_selector_payload(
    selector_str: &str,
) -> AppResult<CoreSelectorPayload> {
    let parsed = parse_face_selector_value(selector_str)?;
    Ok(match parsed {
        FaceSelector::TargetIds(target_ids) => CoreSelectorPayload::FaceTargetIds(target_ids),
        FaceSelector::Clauses(clauses) => CoreSelectorPayload::FaceClauses(
            clauses
                .into_iter()
                .map(core_face_selector_clause_from_face_clause)
                .collect(),
        ),
    })
}

pub(crate) fn edge_selector_spec_from_core_payload(
    payload: &CoreSelectorPayload,
) -> AppResult<EdgeSelectorSpec> {
    match payload {
        CoreSelectorPayload::EdgeAll => parse_edge_selector_spec("all"),
        CoreSelectorPayload::EdgeClauses(clauses) => {
            parse_edge_selector_spec(&core_edge_selector_clauses_string(clauses))
        }
        CoreSelectorPayload::EdgeTargetIds(target_ids) => {
            parse_edge_selector_spec(&format!("target-ids:{}", target_ids.join("|")))
        }
        CoreSelectorPayload::FaceTargetIds(target_ids) => Err(validation(format!(
            "Expected edge selector payload, got face target ids {:?}.",
            target_ids
        ))),
        CoreSelectorPayload::FaceClauses(clauses) => Err(validation(format!(
            "Expected edge selector payload, got face selector clauses {:?}.",
            clauses
        ))),
    }
}

pub(crate) fn face_selector_spec_from_core_payload(
    payload: &CoreSelectorPayload,
) -> AppResult<FaceSelectorSpec> {
    match payload {
        CoreSelectorPayload::FaceClauses(clauses) => {
            parse_face_selector_spec(&core_face_selector_clauses_string(clauses))
        }
        CoreSelectorPayload::FaceTargetIds(target_ids) => {
            parse_face_selector_spec(&format!("target-ids:{}", target_ids.join("|")))
        }
        CoreSelectorPayload::EdgeAll => Err(validation(
            "Expected face selector payload, got edge selector `all`.",
        )),
        CoreSelectorPayload::EdgeClauses(clauses) => Err(validation(format!(
            "Expected face selector payload, got edge selector clauses {:?}.",
            clauses
        ))),
        CoreSelectorPayload::EdgeTargetIds(target_ids) => Err(validation(format!(
            "Expected face selector payload, got edge target ids {:?}.",
            target_ids
        ))),
    }
}

pub(crate) fn parse_face_selector_spec(selector_str: &str) -> AppResult<FaceSelectorSpec> {
    let parsed = parse_face_selector_value(selector_str)?;
    Ok(FaceSelectorSpec {
        canonical_string: parsed.canonical_string(),
        python_payload_literal: face_selector_python_payload_literal(&parsed),
        target_ids: parsed.target_ids().to_vec(),
    })
}

pub(crate) fn exact_edge_target_ids_from_selector_str(
    selector_str: &str,
) -> AppResult<Option<Vec<String>>> {
    exact_target_ids_from_prefixed_selector_str(selector_str, ":edge:", "Edge")
}

pub(crate) fn exact_face_target_ids_from_selector_str(
    selector_str: &str,
) -> AppResult<Option<Vec<String>>> {
    exact_target_ids_from_prefixed_selector_str(selector_str, ":face:", "Face")
}

fn exact_target_ids_from_prefixed_selector_str(
    selector_str: &str,
    required_marker: &str,
    selector_kind: &str,
) -> AppResult<Option<Vec<String>>> {
    let raw = selector_str.trim();
    if raw.is_empty() {
        return Ok(None);
    }

    let lower = raw.to_ascii_lowercase();
    let payload = if let Some(rest) = raw.get(prefix_len_if_matches(&lower, "target-id:")..) {
        Some(rest)
    } else if let Some(rest) = raw.get(prefix_len_if_matches(&lower, "target-ids:")..) {
        Some(rest)
    } else {
        None
    };

    let Some(payload) = payload else {
        return Ok(None);
    };
    let target_ids = split_target_ids(payload);
    if target_ids.is_empty() {
        return Err(validation(format!(
            "{selector_kind} selector `{}` did not include any target ids.",
            selector_str
        )));
    }
    if let Some(invalid_target_id) = target_ids
        .iter()
        .find(|target_id| !target_id.contains(required_marker))
    {
        return Err(validation(format!(
            "{selector_kind} selector `{}` included non-{} target id `{}`.",
            selector_str,
            selector_kind.to_ascii_lowercase(),
            invalid_target_id
        )));
    }
    Ok(Some(target_ids))
}

fn prefix_len_if_matches(value: &str, prefix: &str) -> usize {
    if value.starts_with(prefix) {
        prefix.len()
    } else {
        usize::MAX
    }
}

fn split_target_ids(payload: &str) -> Vec<String> {
    let separator = if payload.contains('|') {
        '|'
    } else if payload.contains(',') {
        ','
    } else {
        '\0'
    };
    let raw_items: Vec<&str> = if separator == '\0' {
        vec![payload]
    } else {
        payload.split(separator).collect()
    };

    let mut target_ids = Vec::new();
    for raw_item in raw_items {
        let item = raw_item.trim();
        if item.is_empty() {
            continue;
        }
        if !target_ids.iter().any(|existing| existing == item) {
            target_ids.push(item.to_string());
        }
    }
    target_ids
}

fn edge_selector_python_payload_literal(selector: &EdgeSelector) -> String {
    match selector {
        EdgeSelector::All => "{'kind': 'all'}".to_string(),
        EdgeSelector::TargetIds(target_ids) => format!(
            "{{'kind': 'targetIds', 'targetIds': [{}]}}",
            python_string_list_literal(target_ids)
        ),
        _ => {
            let clauses = selector.clauses().unwrap_or_default();
            let clause_literals = clauses
                .iter()
                .map(|clause| match clause {
                    EdgeSelectorClause::Axis(axis) => {
                        format!("{{'kind': 'axis', 'axis': '{}'}}", edge_axis_name(*axis))
                    }
                    EdgeSelectorClause::Boundary { axis, bound } => format!(
                        "{{'kind': 'boundary', 'axis': '{}', 'bound': '{}'}}",
                        edge_axis_name(*axis),
                        edge_bound_name(*bound)
                    ),
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{'kind': 'clauses', 'clauses': [{clause_literals}]}}")
        }
    }
}

fn face_selector_python_payload_literal(selector: &FaceSelector) -> String {
    match selector {
        FaceSelector::Clauses(clauses) => {
            if clauses.is_empty() {
                "{'kind': 'all'}".to_string()
            } else {
                let clause_literals = clauses
                    .iter()
                    .map(|clause| match clause {
                        FaceSelectorClause::Boundary { axis, bound } => format!(
                            "{{'kind': 'boundary', 'axis': '{}', 'bound': '{}'}}",
                            edge_axis_name(*axis),
                            edge_bound_name(*bound)
                        ),
                        FaceSelectorClause::Planar => "{'kind': 'planar'}".to_string(),
                        FaceSelectorClause::Normal(axis) => {
                            format!("{{'kind': 'normal', 'axis': '{}'}}", edge_axis_name(*axis))
                        }
                        FaceSelectorClause::Area(rank) => match rank {
                            FaceAreaRank::Min => "{'kind': 'area', 'rank': 'min'}".to_string(),
                            FaceAreaRank::Max => "{'kind': 'area', 'rank': 'max'}".to_string(),
                        },
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{{'kind': 'clauses', 'clauses': [{clause_literals}]}}")
            }
        }
        FaceSelector::TargetIds(target_ids) => format!(
            "{{'kind': 'targetIds', 'targetIds': [{}]}}",
            python_string_list_literal(target_ids)
        ),
    }
}

fn parse_face_selector_clause(token: &str, full_selector: &str) -> AppResult<FaceSelectorClause> {
    match token {
        "top" => {
            return Ok(FaceSelectorClause::Boundary {
                axis: EdgeAxis::Z,
                bound: EdgeBound::Max,
            })
        }
        "bottom" => {
            return Ok(FaceSelectorClause::Boundary {
                axis: EdgeAxis::Z,
                bound: EdgeBound::Min,
            })
        }
        "left" => {
            return Ok(FaceSelectorClause::Boundary {
                axis: EdgeAxis::X,
                bound: EdgeBound::Min,
            })
        }
        "right" => {
            return Ok(FaceSelectorClause::Boundary {
                axis: EdgeAxis::X,
                bound: EdgeBound::Max,
            })
        }
        "front" => {
            return Ok(FaceSelectorClause::Boundary {
                axis: EdgeAxis::Y,
                bound: EdgeBound::Max,
            })
        }
        "back" => {
            return Ok(FaceSelectorClause::Boundary {
                axis: EdgeAxis::Y,
                bound: EdgeBound::Min,
            })
        }
        "planar" => return Ok(FaceSelectorClause::Planar),
        _ => {}
    }
    if let Some(axis_name) = token.strip_prefix("normal-") {
        return Ok(FaceSelectorClause::Normal(parse_edge_axis(
            axis_name,
            full_selector,
        )?));
    }
    if let Some(rank_name) = token.strip_prefix("area-") {
        let rank = match rank_name {
            "min" => FaceAreaRank::Min,
            "max" => FaceAreaRank::Max,
            _ => {
                return Err(validation(format!(
                    "Unknown face selector `{}`. Use {}",
                    full_selector, FACE_SELECTOR_HELP
                )))
            }
        };
        return Ok(FaceSelectorClause::Area(rank));
    }
    if let Some((axis_name, bound_name)) = token.split_once('-') {
        let axis = parse_edge_axis(axis_name, full_selector)?;
        let bound = match bound_name {
            "min" => EdgeBound::Min,
            "max" => EdgeBound::Max,
            _ => {
                return Err(validation(format!(
                    "Unknown face selector `{}`. Use {}",
                    full_selector, FACE_SELECTOR_HELP
                )))
            }
        };
        return Ok(FaceSelectorClause::Boundary { axis, bound });
    }
    Err(validation(format!(
        "Unknown face selector `{}`. Use {}",
        full_selector, FACE_SELECTOR_HELP
    )))
}

fn core_edge_selector_clause_from_edge_clause(
    clause: EdgeSelectorClause,
) -> CoreEdgeSelectorClause {
    match clause {
        EdgeSelectorClause::Axis(axis) => {
            CoreEdgeSelectorClause::Axis(core_edge_axis_from_edge_axis(axis))
        }
        EdgeSelectorClause::Boundary { axis, bound } => CoreEdgeSelectorClause::Boundary {
            axis: core_edge_axis_from_edge_axis(axis),
            bound: core_edge_bound_from_edge_bound(bound),
        },
    }
}

fn core_face_selector_clause_from_face_clause(
    clause: FaceSelectorClause,
) -> CoreFaceSelectorClause {
    match clause {
        FaceSelectorClause::Boundary { axis, bound } => CoreFaceSelectorClause::Boundary {
            axis: core_edge_axis_from_edge_axis(axis),
            bound: core_edge_bound_from_edge_bound(bound),
        },
        FaceSelectorClause::Planar => CoreFaceSelectorClause::Planar,
        FaceSelectorClause::Normal(axis) => {
            CoreFaceSelectorClause::Normal(core_edge_axis_from_edge_axis(axis))
        }
        FaceSelectorClause::Area(rank) => CoreFaceSelectorClause::Area(match rank {
            FaceAreaRank::Min => CoreFaceAreaRank::Min,
            FaceAreaRank::Max => CoreFaceAreaRank::Max,
        }),
    }
}

fn core_edge_axis_from_edge_axis(axis: EdgeAxis) -> CoreEdgeAxis {
    match axis {
        EdgeAxis::X => CoreEdgeAxis::X,
        EdgeAxis::Y => CoreEdgeAxis::Y,
        EdgeAxis::Z => CoreEdgeAxis::Z,
    }
}

fn core_edge_bound_from_edge_bound(bound: EdgeBound) -> CoreEdgeBound {
    match bound {
        EdgeBound::Min => CoreEdgeBound::Min,
        EdgeBound::Max => CoreEdgeBound::Max,
    }
}

fn edge_axis_from_core_edge_axis(axis: CoreEdgeAxis) -> EdgeAxis {
    match axis {
        CoreEdgeAxis::X => EdgeAxis::X,
        CoreEdgeAxis::Y => EdgeAxis::Y,
        CoreEdgeAxis::Z => EdgeAxis::Z,
    }
}

fn edge_bound_from_core_edge_bound(bound: CoreEdgeBound) -> EdgeBound {
    match bound {
        CoreEdgeBound::Min => EdgeBound::Min,
        CoreEdgeBound::Max => EdgeBound::Max,
    }
}

fn face_area_rank_from_core(rank: CoreFaceAreaRank) -> FaceAreaRank {
    match rank {
        CoreFaceAreaRank::Min => FaceAreaRank::Min,
        CoreFaceAreaRank::Max => FaceAreaRank::Max,
    }
}

fn core_edge_selector_clauses_string(clauses: &[CoreEdgeSelectorClause]) -> String {
    if clauses.is_empty() {
        return "all".to_string();
    }
    clauses
        .iter()
        .map(|clause| match clause {
            CoreEdgeSelectorClause::Axis(axis) => {
                format!(
                    "axis-{}",
                    edge_axis_name(edge_axis_from_core_edge_axis(*axis))
                )
            }
            CoreEdgeSelectorClause::Boundary { axis, bound } => format!(
                "{}-{}",
                edge_axis_name(edge_axis_from_core_edge_axis(*axis)),
                edge_bound_name(edge_bound_from_core_edge_bound(*bound))
            ),
        })
        .collect::<Vec<_>>()
        .join("+")
}

fn core_face_selector_clauses_string(clauses: &[CoreFaceSelectorClause]) -> String {
    if clauses.is_empty() {
        return "all".to_string();
    }
    clauses
        .iter()
        .map(|clause| match clause {
            CoreFaceSelectorClause::Boundary { axis, bound } => format!(
                "{}-{}",
                edge_axis_name(edge_axis_from_core_edge_axis(*axis)),
                edge_bound_name(edge_bound_from_core_edge_bound(*bound))
            ),
            CoreFaceSelectorClause::Planar => "planar".to_string(),
            CoreFaceSelectorClause::Normal(axis) => {
                format!(
                    "normal-{}",
                    edge_axis_name(edge_axis_from_core_edge_axis(*axis))
                )
            }
            CoreFaceSelectorClause::Area(rank) => match face_area_rank_from_core(*rank) {
                FaceAreaRank::Min => "area-min".to_string(),
                FaceAreaRank::Max => "area-max".to_string(),
            },
        })
        .collect::<Vec<_>>()
        .join("+")
}

fn python_string_list_literal(items: &[String]) -> String {
    items
        .iter()
        .map(|item| format!("{item:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn parse_edge_selector_clause(token: &str, full_selector: &str) -> AppResult<EdgeSelectorClause> {
    match token {
        "top" => {
            return Ok(EdgeSelectorClause::Boundary {
                axis: EdgeAxis::Z,
                bound: EdgeBound::Max,
            })
        }
        "bottom" => {
            return Ok(EdgeSelectorClause::Boundary {
                axis: EdgeAxis::Z,
                bound: EdgeBound::Min,
            })
        }
        "left" => {
            return Ok(EdgeSelectorClause::Boundary {
                axis: EdgeAxis::X,
                bound: EdgeBound::Min,
            })
        }
        "right" => {
            return Ok(EdgeSelectorClause::Boundary {
                axis: EdgeAxis::X,
                bound: EdgeBound::Max,
            })
        }
        "front" => {
            return Ok(EdgeSelectorClause::Boundary {
                axis: EdgeAxis::Y,
                bound: EdgeBound::Max,
            })
        }
        "back" => {
            return Ok(EdgeSelectorClause::Boundary {
                axis: EdgeAxis::Y,
                bound: EdgeBound::Min,
            })
        }
        "vertical" => return Ok(EdgeSelectorClause::Axis(EdgeAxis::Z)),
        _ => {}
    }
    if let Some(axis_name) = token.strip_prefix("axis-") {
        let axis = parse_edge_axis(axis_name, full_selector)?;
        return Ok(EdgeSelectorClause::Axis(axis));
    }
    if let Some((axis_name, bound_name)) = token.split_once('-') {
        let axis = parse_edge_axis(axis_name, full_selector)?;
        let bound = match bound_name {
            "min" => EdgeBound::Min,
            "max" => EdgeBound::Max,
            _ => {
                return Err(validation(format!(
                    "Unknown edge selector `{}`. Use {}",
                    full_selector, EDGE_SELECTOR_HELP
                )))
            }
        };
        return Ok(EdgeSelectorClause::Boundary { axis, bound });
    }
    Err(validation(format!(
        "Unknown edge selector `{}`. Use {}",
        full_selector, EDGE_SELECTOR_HELP
    )))
}

fn parse_edge_axis(axis_name: &str, full_selector: &str) -> AppResult<EdgeAxis> {
    match axis_name {
        "x" => Ok(EdgeAxis::X),
        "y" => Ok(EdgeAxis::Y),
        "z" => Ok(EdgeAxis::Z),
        _ => Err(validation(format!(
            "Unknown edge selector `{}`. Use {}",
            full_selector, EDGE_SELECTOR_HELP
        ))),
    }
}

fn edge_axis_name(axis: EdgeAxis) -> &'static str {
    match axis {
        EdgeAxis::X => "x",
        EdgeAxis::Y => "y",
        EdgeAxis::Z => "z",
    }
}

fn edge_bound_name(bound: EdgeBound) -> &'static str {
    match bound {
        EdgeBound::Min => "min",
        EdgeBound::Max => "max",
    }
}

pub(super) fn detect_feature_edges(mesh: &IrMesh) -> Vec<FeatureEdge> {
    let tri_mesh = mesh.triangulate();
    let polygons = &tri_mesh.polygons;

    // Build vertex index map for epsilon-based deduplication.
    let mut vertex_map = csgrs::mesh::connectivity::VertexIndexMap::new(1e-9);
    for poly in polygons {
        for v in &poly.vertices {
            vertex_map.get_or_create_index(v.pos);
        }
    }

    // Map each canonical edge to the (up to two) polygon indices sharing it.
    let mut edge_faces: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (poly_idx, poly) in polygons.iter().enumerate() {
        let verts: Vec<usize> = poly
            .vertices
            .iter()
            .map(|v| vertex_map.get_or_create_index(v.pos))
            .collect();
        for i in 0..verts.len() {
            let a = verts[i];
            let b = verts[(i + 1) % verts.len()];
            let key = if a < b { (a, b) } else { (b, a) };
            edge_faces.entry(key).or_default().push(poly_idx);
        }
    }

    let mut result = Vec::new();
    for ((vi_a, vi_b), faces) in &edge_faces {
        if faces.len() != 2 {
            continue; // Non-manifold or boundary edge — skip.
        }
        let n1 = polygons[faces[0]].plane.normal();
        let n2 = polygons[faces[1]].plane.normal();
        let dot = n1.dot(&n2).clamp(-1.0, 1.0);
        let dihedral = dot.acos();
        if dihedral < FEATURE_EDGE_DIHEDRAL_THRESHOLD {
            continue; // Nearly coplanar — not a feature edge.
        }
        let pos_a = vertex_map
            .get_position(*vi_a)
            .expect("vertex index must exist");
        let pos_b = vertex_map
            .get_position(*vi_b)
            .expect("vertex index must exist");
        result.push(FeatureEdge {
            vi_a: *vi_a,
            vi_b: *vi_b,
            pos_a,
            pos_b,
            normal_left: n1,
            normal_right: n2,
            dihedral,
        });
    }
    result
}

pub(super) fn filter_edges(edges: &[FeatureEdge], selector: EdgeSelector) -> Vec<&FeatureEdge> {
    let Some(clauses) = selector.clauses() else {
        return edges.iter().collect();
    };
    let extrema = edge_extrema(edges);
    edges
        .iter()
        .filter(|edge| {
            clauses
                .iter()
                .all(|clause| edge_matches_clause(edge, *clause, extrema))
        })
        .collect()
}

fn edge_extrema(edges: &[FeatureEdge]) -> EdgeExtrema {
    let min_x = edges
        .iter()
        .map(|e| e.pos_a.x.min(e.pos_b.x))
        .fold(f64::INFINITY, f64::min);
    let max_x = edges
        .iter()
        .map(|e| e.pos_a.x.max(e.pos_b.x))
        .fold(f64::NEG_INFINITY, f64::max);
    let min_y = edges
        .iter()
        .map(|e| e.pos_a.y.min(e.pos_b.y))
        .fold(f64::INFINITY, f64::min);
    let max_y = edges
        .iter()
        .map(|e| e.pos_a.y.max(e.pos_b.y))
        .fold(f64::NEG_INFINITY, f64::max);
    let min_z = edges
        .iter()
        .map(|e| e.pos_a.z.min(e.pos_b.z))
        .fold(f64::INFINITY, f64::min);
    let max_z = edges
        .iter()
        .map(|e| e.pos_a.z.max(e.pos_b.z))
        .fold(f64::NEG_INFINITY, f64::max);
    let span = (max_x - min_x)
        .abs()
        .max((max_y - min_y).abs())
        .max((max_z - min_z).abs())
        .max(1.0);
    EdgeExtrema {
        min_x,
        max_x,
        min_y,
        max_y,
        min_z,
        max_z,
        tol: span * 1e-6,
    }
}

fn edge_matches_clause(
    edge: &FeatureEdge,
    clause: EdgeSelectorClause,
    extrema: EdgeExtrema,
) -> bool {
    match clause {
        EdgeSelectorClause::Axis(axis) => edge_matches_axis(edge, axis, extrema.tol),
        EdgeSelectorClause::Boundary { axis, bound } => {
            edge_matches_boundary(edge, axis, bound, extrema.tol, extrema)
        }
    }
}

fn edge_matches_axis(edge: &FeatureEdge, axis: EdgeAxis, tol: f64) -> bool {
    let dx = (edge.pos_b.x - edge.pos_a.x).abs();
    let dy = (edge.pos_b.y - edge.pos_a.y).abs();
    let dz = (edge.pos_b.z - edge.pos_a.z).abs();
    match axis {
        EdgeAxis::X => dx > tol && dy <= tol && dz <= tol,
        EdgeAxis::Y => dy > tol && dx <= tol && dz <= tol,
        EdgeAxis::Z => dz > tol && dx <= tol && dy <= tol,
    }
}

fn edge_matches_boundary(
    edge: &FeatureEdge,
    axis: EdgeAxis,
    bound: EdgeBound,
    tol: f64,
    extrema: EdgeExtrema,
) -> bool {
    let (a, b, target) = match (axis, bound) {
        (EdgeAxis::X, EdgeBound::Min) => (edge.pos_a.x, edge.pos_b.x, extrema.min_x),
        (EdgeAxis::X, EdgeBound::Max) => (edge.pos_a.x, edge.pos_b.x, extrema.max_x),
        (EdgeAxis::Y, EdgeBound::Min) => (edge.pos_a.y, edge.pos_b.y, extrema.min_y),
        (EdgeAxis::Y, EdgeBound::Max) => (edge.pos_a.y, edge.pos_b.y, extrema.max_y),
        (EdgeAxis::Z, EdgeBound::Min) => (edge.pos_a.z, edge.pos_b.z, extrema.min_z),
        (EdgeAxis::Z, EdgeBound::Max) => (edge.pos_a.z, edge.pos_b.z, extrema.max_z),
    };
    (a - target).abs() <= tol && (b - target).abs() <= tol
}

pub(super) fn chamfer_mesh(
    mesh: &IrMesh,
    distance: f64,
    selector: EdgeSelector,
) -> AppResult<IrMesh> {
    if selector.target_ids().is_some() {
        return Err(validation(
            "Exact edge target-id selectors are not supported by the EckyRust mesh fallback. Use an exact backend.",
        ));
    }
    if distance.abs() < 1e-9 {
        return Ok(mesh.clone());
    }
    let tri_mesh = mesh.triangulate();
    let all_edges = detect_feature_edges(&tri_mesh);
    let selected = filter_edges(&all_edges, selector);
    if selected.is_empty() {
        return Ok(mesh.clone());
    }

    // Build vertex index map matching the one used in detect_feature_edges.
    let polygons = &tri_mesh.polygons;
    let mut vertex_map = csgrs::mesh::connectivity::VertexIndexMap::new(1e-9);
    for poly in polygons {
        for v in &poly.vertices {
            vertex_map.get_or_create_index(v.pos);
        }
    }

    // Collect the set of selected edge keys.
    let selected_keys: std::collections::HashSet<(usize, usize)> =
        selected.iter().map(|e| (e.vi_a, e.vi_b)).collect();

    // For each selected edge, build a cutting plane that bevels the edge.
    // Strategy: for each triangle, if it has an edge in the selected set,
    // inset the edge by `distance` along the face plane and produce a chamfer
    // strip connecting the two faces.

    // Build edge → face normal pairs for selected edges.
    let mut edge_normals: HashMap<(usize, usize), (Vector3<f64>, Vector3<f64>)> = HashMap::new();
    for e in &selected {
        edge_normals.insert((e.vi_a, e.vi_b), (e.normal_left, e.normal_right));
    }

    // For each polygon, find which of its edges are selected and split accordingly.
    let mut out_polygons: Vec<IrPolygon<()>> = Vec::new();
    let mut chamfer_strips: Vec<[Point3<f64>; 4]> = Vec::new();

    // Track which polygon index was "left" or "right" for each edge so we
    // can assign inset directions consistently.
    let mut edge_face_sides: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (poly_idx, poly) in polygons.iter().enumerate() {
        let verts: Vec<usize> = poly
            .vertices
            .iter()
            .map(|v| vertex_map.get_or_create_index(v.pos))
            .collect();
        for i in 0..verts.len() {
            let a = verts[i];
            let b = verts[(i + 1) % verts.len()];
            let key = if a < b { (a, b) } else { (b, a) };
            if selected_keys.contains(&key) {
                edge_face_sides.entry(key).or_default().push(poly_idx);
            }
        }
    }

    // For each polygon, check if any of its edges are chamfered.
    // If so, inset those edge vertices along the face plane.
    for (poly_idx, poly) in polygons.iter().enumerate() {
        let face_normal = poly.plane.normal();
        let verts: Vec<usize> = poly
            .vertices
            .iter()
            .map(|v| vertex_map.get_or_create_index(v.pos))
            .collect();

        let mut has_chamfer_edge = false;
        let mut new_vertices: Vec<IrVertex> = Vec::new();

        for i in 0..verts.len() {
            let a = verts[i];
            let b = verts[(i + 1) % verts.len()];
            let key = if a < b { (a, b) } else { (b, a) };

            new_vertices.push(poly.vertices[i].clone());

            if selected_keys.contains(&key) {
                has_chamfer_edge = true;

                let pos_a = poly.vertices[i].pos;
                let pos_b = poly.vertices[(i + 1) % verts.len()].pos;
                let edge_dir = (pos_b - pos_a).normalize();

                // Inset direction: perpendicular to the edge within the face plane.
                let inset_dir = face_normal.cross(&edge_dir).normalize();
                // Ensure inset points inward (toward face interior).
                let face_center = Point3::from(
                    poly.vertices
                        .iter()
                        .fold(Vector3::zeros(), |acc, v| acc + v.pos.coords)
                        / poly.vertices.len() as f64,
                );
                let test_point = pos_a + inset_dir * 0.001;
                let inset_dir = if (test_point - face_center).norm() < (pos_a - face_center).norm()
                {
                    inset_dir
                } else {
                    -inset_dir
                };

                let inset_a = pos_a + inset_dir * distance;
                let inset_b = pos_b + inset_dir * distance;

                // Replace the original edge endpoints with inset versions.
                // We add the inset points and will later form the chamfer strip.
                let last_idx = new_vertices.len() - 1;
                new_vertices[last_idx] = IrVertex::new(inset_a, face_normal);
                new_vertices.push(IrVertex::new(inset_b, face_normal));

                // Record the chamfer strip quad: original → inset on both sides.
                // We only record from one side; the strip connects when both faces
                // have been processed. Use edge_face_sides to determine if this is
                // the first or second face.
                let sides = edge_face_sides.get(&key).unwrap();
                if sides.len() == 2 && sides[0] == poly_idx {
                    // First face records the strip — we'll get the other side's
                    // inset points from the second face processing below.
                    chamfer_strips.push([pos_a, pos_b, inset_b, inset_a]);
                }
            }
        }

        if has_chamfer_edge && new_vertices.len() >= 3 {
            out_polygons.push(IrPolygon::new(new_vertices, None));
        } else {
            out_polygons.push(poly.clone());
        }
    }

    // Now build the chamfer strip polygons connecting the two inset faces.
    // For each selected edge, we need to form a quad from the inset points
    // on both adjacent faces.
    for e in &selected {
        let key = (e.vi_a, e.vi_b);
        let sides = match edge_face_sides.get(&key) {
            Some(s) if s.len() == 2 => s,
            _ => continue,
        };

        // Get inset positions from both faces.
        let poly_l = &polygons[sides[0]];
        let poly_r = &polygons[sides[1]];
        let nl = poly_l.plane.normal();
        let nr = poly_r.plane.normal();

        let mut compute_inset =
            |poly: &IrPolygon<()>, normal: Vector3<f64>| -> (Point3<f64>, Point3<f64>) {
                let verts: Vec<usize> = poly
                    .vertices
                    .iter()
                    .map(|v| vertex_map.get_or_create_index(v.pos))
                    .collect();
                // Find the edge in this polygon.
                for i in 0..verts.len() {
                    let a = verts[i];
                    let b = verts[(i + 1) % verts.len()];
                    let k = if a < b { (a, b) } else { (b, a) };
                    if k == key {
                        let pos_a = poly.vertices[i].pos;
                        let pos_b = poly.vertices[(i + 1) % verts.len()].pos;
                        let edge_dir = (pos_b - pos_a).normalize();
                        let mut inset_dir = normal.cross(&edge_dir).normalize();
                        let face_center = Point3::from(
                            poly.vertices
                                .iter()
                                .fold(Vector3::zeros(), |acc, v| acc + v.pos.coords)
                                / poly.vertices.len() as f64,
                        );
                        let test_point = pos_a + inset_dir * 0.001;
                        if (test_point - face_center).norm() > (pos_a - face_center).norm() {
                            inset_dir = -inset_dir;
                        }
                        return (pos_a + inset_dir * distance, pos_b + inset_dir * distance);
                    }
                }
                (e.pos_a, e.pos_b) // fallback — shouldn't happen
            };

        let (inset_la, inset_lb) = compute_inset(poly_l, nl);
        let (inset_ra, inset_rb) = compute_inset(poly_r, nr);

        // The chamfer strip quad connects:
        //   inset_la — inset_lb — inset_rb — inset_ra
        // But we need to be careful about winding order for correct normals.
        let edge_vec: Vector3<f64> = inset_lb - inset_la;
        let side_vec: Vector3<f64> = inset_ra - inset_la;
        let chamfer_normal = edge_vec.cross(&side_vec).normalize();
        // Check if normal points outward (same hemisphere as average of face normals).
        let avg_outward = (nl + nr).normalize();
        let strip_verts = if chamfer_normal.dot(&avg_outward) > 0.0 {
            vec![
                IrVertex::new(inset_la, chamfer_normal),
                IrVertex::new(inset_lb, chamfer_normal),
                IrVertex::new(inset_rb, chamfer_normal),
                IrVertex::new(inset_ra, chamfer_normal),
            ]
        } else {
            let flipped = -chamfer_normal;
            vec![
                IrVertex::new(inset_ra, flipped),
                IrVertex::new(inset_rb, flipped),
                IrVertex::new(inset_lb, flipped),
                IrVertex::new(inset_la, flipped),
            ]
        };
        out_polygons.push(IrPolygon::new(strip_verts, None));
    }

    Ok(IrMesh::from_polygons(&out_polygons, None))
}

pub(super) fn polygon_inset_for_edge(
    poly: &IrPolygon<()>,
    key: (usize, usize),
    vertex_map: &mut csgrs::mesh::connectivity::VertexIndexMap,
    distance: f64,
) -> Option<(Point3<f64>, Point3<f64>, Vector3<f64>)> {
    let verts: Vec<usize> = poly
        .vertices
        .iter()
        .map(|v| vertex_map.get_or_create_index(v.pos))
        .collect();
    for i in 0..verts.len() {
        let a = verts[i];
        let b = verts[(i + 1) % verts.len()];
        let k = if a < b { (a, b) } else { (b, a) };
        if k != key {
            continue;
        }
        let pos_a = poly.vertices[i].pos;
        let pos_b = poly.vertices[(i + 1) % verts.len()].pos;
        let edge_dir = (pos_b - pos_a).normalize();
        let face_normal = poly.plane.normal();
        let mut inset_dir = face_normal.cross(&edge_dir).normalize();
        let face_center = Point3::from(
            poly.vertices
                .iter()
                .fold(Vector3::zeros(), |acc, v| acc + v.pos.coords)
                / poly.vertices.len() as f64,
        );
        let test_point = pos_a + inset_dir * 0.001;
        if (test_point - face_center).norm() > (pos_a - face_center).norm() {
            inset_dir = -inset_dir;
        }
        return Some((
            pos_a + inset_dir * distance,
            pos_b + inset_dir * distance,
            inset_dir,
        ));
    }
    None
}

pub(super) fn fillet_inset_distance(radius: f64, dihedral: f64) -> f64 {
    let half = (dihedral * 0.5).clamp(1e-4, std::f64::consts::FRAC_PI_2 - 1e-4);
    radius / half.tan()
}

pub(super) fn fillet_segment_count(radius: f64, dihedral: f64) -> usize {
    let density = (radius.abs() * dihedral.max(0.5)).ceil() as usize;
    density.clamp(4, 10)
}

pub(super) fn rotate_around_axis(v: Vector3<f64>, axis: Vector3<f64>, angle: f64) -> Vector3<f64> {
    let unit_axis = axis.normalize();
    let cos = angle.cos();
    let sin = angle.sin();
    v * cos + unit_axis.cross(&v) * sin + unit_axis * unit_axis.dot(&v) * (1.0 - cos)
}

pub(super) fn fillet_arc_points(
    corner: Point3<f64>,
    start_dir: Vector3<f64>,
    end_dir: Vector3<f64>,
    axis: Vector3<f64>,
    radius: f64,
    dihedral: f64,
    segments: usize,
) -> Option<Vec<(Point3<f64>, Vector3<f64>)>> {
    let bisector = (start_dir + end_dir).try_normalize(1e-9)?;
    let center = corner + bisector * (radius / (dihedral * 0.5).sin());
    let tangent_distance = fillet_inset_distance(radius, dihedral);
    let tangent_start = corner + start_dir * tangent_distance;
    let tangent_end = corner + end_dir * tangent_distance;
    let radial_start = tangent_start - center;
    let radial_end = tangent_end - center;
    let axis = axis.try_normalize(1e-9)?;
    let mut angle = radial_start
        .normalize()
        .dot(&radial_end.normalize())
        .clamp(-1.0, 1.0)
        .acos();
    if radial_start.cross(&radial_end).dot(&axis) < 0.0 {
        angle = -angle;
    }
    let mut points = Vec::with_capacity(segments + 1);
    for step in 0..=segments {
        let t = step as f64 / segments as f64;
        let radial = rotate_around_axis(radial_start, axis, angle * t);
        let point = center + radial;
        points.push((point, radial.normalize()));
    }
    Some(points)
}

pub(super) fn fillet_mesh(mesh: &IrMesh, radius: f64, selector: EdgeSelector) -> AppResult<IrMesh> {
    if selector.target_ids().is_some() {
        return Err(validation(
            "Exact edge target-id selectors are not supported by the EckyRust mesh fallback. Use an exact backend.",
        ));
    }
    if radius.abs() < 1e-9 {
        return Ok(mesh.clone());
    }
    let tri_mesh = mesh.triangulate();
    let all_edges = detect_feature_edges(&tri_mesh);
    let selected = filter_edges(&all_edges, selector);
    if selected.is_empty() {
        return Ok(mesh.clone());
    }

    let polygons = &tri_mesh.polygons;
    let mut vertex_map = csgrs::mesh::connectivity::VertexIndexMap::new(1e-9);
    for poly in polygons {
        for v in &poly.vertices {
            vertex_map.get_or_create_index(v.pos);
        }
    }

    let edge_distances: HashMap<(usize, usize), f64> = selected
        .iter()
        .map(|edge| {
            (
                (edge.vi_a, edge.vi_b),
                fillet_inset_distance(radius, edge.dihedral),
            )
        })
        .collect();
    let selected_keys: std::collections::HashSet<(usize, usize)> =
        edge_distances.keys().copied().collect();

    let mut edge_face_sides: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (poly_idx, poly) in polygons.iter().enumerate() {
        let verts: Vec<usize> = poly
            .vertices
            .iter()
            .map(|v| vertex_map.get_or_create_index(v.pos))
            .collect();
        for i in 0..verts.len() {
            let a = verts[i];
            let b = verts[(i + 1) % verts.len()];
            let key = if a < b { (a, b) } else { (b, a) };
            if selected_keys.contains(&key) {
                edge_face_sides.entry(key).or_default().push(poly_idx);
            }
        }
    }

    let mut out_polygons: Vec<IrPolygon<()>> = Vec::new();
    for poly in polygons {
        let verts: Vec<usize> = poly
            .vertices
            .iter()
            .map(|v| vertex_map.get_or_create_index(v.pos))
            .collect();
        let face_normal = poly.plane.normal();
        let mut modified = false;
        let mut new_vertices = Vec::new();

        for i in 0..verts.len() {
            let a = verts[i];
            let b = verts[(i + 1) % verts.len()];
            let key = if a < b { (a, b) } else { (b, a) };
            new_vertices.push(poly.vertices[i].clone());
            let Some(distance) = edge_distances.get(&key).copied() else {
                continue;
            };
            modified = true;
            let Some((inset_a, inset_b, _)) =
                polygon_inset_for_edge(poly, key, &mut vertex_map, distance)
            else {
                continue;
            };
            let last_idx = new_vertices.len() - 1;
            new_vertices[last_idx] = IrVertex::new(inset_a, face_normal);
            new_vertices.push(IrVertex::new(inset_b, face_normal));
        }

        if modified && new_vertices.len() >= 3 {
            out_polygons.push(IrPolygon::new(new_vertices, None));
        } else {
            out_polygons.push(poly.clone());
        }
    }

    for edge in selected {
        let key = (edge.vi_a, edge.vi_b);
        let Some(sides) = edge_face_sides.get(&key) else {
            continue;
        };
        if sides.len() != 2 {
            continue;
        }
        let Some(distance) = edge_distances.get(&key).copied() else {
            continue;
        };
        let poly_l = &polygons[sides[0]];
        let poly_r = &polygons[sides[1]];
        let Some((_inset_la, _inset_lb, dir_l)) =
            polygon_inset_for_edge(poly_l, key, &mut vertex_map, distance)
        else {
            continue;
        };
        let Some((_inset_ra, _inset_rb, dir_r)) =
            polygon_inset_for_edge(poly_r, key, &mut vertex_map, distance)
        else {
            continue;
        };
        let edge_axis = edge.pos_b - edge.pos_a;
        let segments = fillet_segment_count(radius, edge.dihedral);
        let Some(arc_a) = fillet_arc_points(
            edge.pos_a,
            dir_l,
            dir_r,
            edge_axis,
            radius,
            edge.dihedral,
            segments,
        ) else {
            continue;
        };
        let Some(arc_b) = fillet_arc_points(
            edge.pos_b,
            dir_l,
            dir_r,
            edge_axis,
            radius,
            edge.dihedral,
            segments,
        ) else {
            continue;
        };

        let avg_outward = (edge.normal_left + edge.normal_right)
            .try_normalize(1e-9)
            .unwrap_or(edge.normal_left);
        for segment in 0..segments {
            let (a0, n_a0) = arc_a[segment];
            let (a1, n_a1) = arc_a[segment + 1];
            let (b0, n_b0) = arc_b[segment];
            let (b1, n_b1) = arc_b[segment + 1];
            let mut poly = IrPolygon::new(
                vec![
                    IrVertex::new(a0, n_a0),
                    IrVertex::new(b0, n_b0),
                    IrVertex::new(b1, n_b1),
                    IrVertex::new(a1, n_a1),
                ],
                None,
            );
            if poly.plane.normal().dot(&avg_outward) < 0.0 {
                poly.flip();
            }
            out_polygons.push(poly);
        }
    }

    Ok(IrMesh::from_polygons(&out_polygons, None))
}

pub(super) fn parse_edge_selector(
    args: &[IrExpr],
    env: &BTreeMap<String, ParamValue>,
) -> AppResult<(EdgeSelector, usize)> {
    // Check for :edges "selector" keyword pair after the distance argument.
    if args.len() >= 3
        && expr_keyword_name(&args[1])
            .map(|k| k == "edges")
            .unwrap_or(false)
    {
        let selector_str = eval_stringish(&args[2], env)?;
        let selector = parse_edge_selector_value(&selector_str)?;
        Ok((selector, 3))
    } else {
        Ok((EdgeSelector::All, 1))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        exact_edge_target_ids_from_selector_str, exact_face_target_ids_from_selector_str,
        parse_edge_selector_spec, parse_face_selector_spec,
    };

    #[test]
    fn exact_edge_selector_rejects_face_target_ids() {
        let err = exact_edge_target_ids_from_selector_str("target-id:body:face:0:0-0-10:100")
            .expect_err("face target id should fail edge selector");
        assert!(
            err.message
                .contains("included non-edge target id `body:face:0:0-0-10:100`"),
            "{err:?}"
        );
    }

    #[test]
    fn exact_face_selector_rejects_edge_target_ids() {
        let err = exact_face_target_ids_from_selector_str("target-id:body:edge:0:0-0-0_10-0-0")
            .expect_err("edge target id should fail face selector");
        assert!(
            err.message
                .contains("included non-face target id `body:edge:0:0-0-0_10-0-0`"),
            "{err:?}"
        );
    }

    #[test]
    fn exact_edge_selector_rejects_mixed_target_kinds() {
        let err = exact_edge_target_ids_from_selector_str(
            "target-ids:body:edge:0:0-0-0_10-0-0|body:face:0:0-0-10:100",
        )
        .expect_err("mixed selector should fail");
        assert!(
            err.message
                .contains("included non-edge target id `body:face:0:0-0-10:100`"),
            "{err:?}"
        );
    }

    #[test]
    fn exact_face_selector_rejects_bare_raw_target_id() {
        let parsed = exact_face_target_ids_from_selector_str("body:face:0:0-0-10:100")
            .expect("bare raw selector should parse as non-exact");
        assert!(parsed.is_none(), "{parsed:?}");
    }

    #[test]
    fn selector_specs_canonicalize_and_expose_exact_ids() {
        let edge = parse_edge_selector_spec("left+vertical").expect("edge spec");
        assert_eq!(edge.canonical_string(), "x-min+axis-z");
        assert_eq!(
            edge.python_payload_literal(),
            "{'kind': 'clauses', 'clauses': [{'kind': 'boundary', 'axis': 'x', 'bound': 'min'}, {'kind': 'axis', 'axis': 'z'}]}"
        );
        assert!(edge.target_ids().is_none(), "{edge:?}");

        let face = parse_face_selector_spec("target-id:body:face:5:0-0-10:100").expect("face spec");
        assert_eq!(face.canonical_string(), "target-ids:body:face:5:0-0-10:100");
        assert_eq!(
            face.python_payload_literal(),
            "{'kind': 'targetIds', 'targetIds': [\"body:face:5:0-0-10:100\"]}"
        );
        assert_eq!(face.target_ids(), &["body:face:5:0-0-10:100".to_string()]);
    }

    #[test]
    fn face_selector_specs_support_planar_normal_area_compounds() {
        let face = parse_face_selector_spec("planar+normal-z+area-max").expect("face spec");
        assert_eq!(face.canonical_string(), "planar+normal-z+area-max");
        assert_eq!(
            face.python_payload_literal(),
            "{'kind': 'clauses', 'clauses': [{'kind': 'planar'}, {'kind': 'normal', 'axis': 'z'}, {'kind': 'area', 'rank': 'max'}]}"
        );
        assert!(face.target_ids().is_empty(), "{face:?}");
    }
}
