use crate::models::SelectionTarget;

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
    use crate::models::{SelectionTarget, SelectionTargetKind};

    use super::{
        durable_edge_target_id, durable_edge_target_id_for_stable_node_key, durable_face_target_id,
        durable_face_target_id_for_stable_node_key, is_stable_topology_target_id,
        portable_topology_target_id, preferred_public_topology_target_id, stable_edge_target_id,
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
}
