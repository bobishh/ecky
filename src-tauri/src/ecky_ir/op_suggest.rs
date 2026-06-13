//! Nearest-op suggester for Ecky's finite Core IR operation vocabulary.
//!
//! Given an unknown op name, this returns the closest known Core IR op names by
//! Levenshtein edit distance (e.g. `bx` -> `box`). It powers the "did you mean
//! `box`?" hint on `AuthoringReason::UnknownOp` errors (design Decision 5).
//!
//! Pure: no IO, no panics, no `unwrap` on external input. The known-op set is
//! derived from the `CoreOperation` enum (`crate::ecky_core_ir`), the canonical
//! finite Core IR vocabulary — see [`known_op_names`] and [`core_operation_name`].

use crate::ecky_core_ir::{
    CoreArrayOp, CoreBooleanOp, CoreFrameOp, CoreMetaOp, CoreOperation, CorePathOp, CorePrimitive,
    CoreSurfaceOp, CoreTransformOp,
};

/// Most suggestions to return for a single unknown op.
const MAX_SUGGESTIONS: usize = 3;
/// Edit-distance threshold for short names. Longer names get a proportional
/// allowance via [`distance_threshold`] so a one-character slip in a long op
/// name still matches.
const BASE_THRESHOLD: usize = 2;

/// Return the nearest known Core IR op names to `input`, closest first.
///
/// Matches are within an edit-distance threshold (`<= 2`, or proportional to the
/// candidate length for longer names), deduplicated, ordered closest-first with
/// ties broken alphabetically, and capped at [`MAX_SUGGESTIONS`]. Returns an
/// empty `Vec` when nothing is close (e.g. far-off garbage). An exactly-valid op
/// is its own nearest match (distance 0).
pub fn suggest_ops(input: &str) -> Vec<String> {
    let input = input.trim();
    if input.is_empty() {
        return Vec::new();
    }

    let mut scored: Vec<(usize, &'static str)> = known_op_names()
        .into_iter()
        .filter_map(|candidate| {
            let distance = levenshtein(input, candidate);
            (distance <= distance_threshold(candidate)).then_some((distance, candidate))
        })
        .collect();

    // Closest first; ties broken alphabetically for a stable, predictable order.
    scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)));

    scored
        .into_iter()
        .take(MAX_SUGGESTIONS)
        .map(|(_, name)| name.to_string())
        .collect()
}

/// Per-candidate edit-distance threshold. Short names use [`BASE_THRESHOLD`];
/// names longer than 6 chars allow roughly one edit per three characters so a
/// single typo in `intersection` or `linear-array` still resolves without
/// matching unrelated words.
fn distance_threshold(candidate: &str) -> usize {
    let proportional = candidate.chars().count() / 3;
    BASE_THRESHOLD.max(proportional)
}

/// The canonical finite set of Core IR op names.
///
/// Derived from the `CoreOperation` enum via [`core_operation_name`]: each list
/// below enumerates the variants of one sub-enum, so adding or removing a
/// `CoreOperation` variant forces a compile error in [`core_operation_name`]
/// (its `match` is exhaustive) — the name set cannot silently drift from the
/// registry. `CoreOperation::Custom` is intentionally excluded: it is the open
/// escape hatch, not part of the finite vocabulary we suggest against.
fn known_op_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = Vec::new();

    let primitives = [
        CorePrimitive::Box,
        CorePrimitive::Sphere,
        CorePrimitive::Cylinder,
        CorePrimitive::Cone,
        CorePrimitive::Torus,
        CorePrimitive::Wedge,
        CorePrimitive::Circle,
        CorePrimitive::Ellipse,
        CorePrimitive::Slot,
        CorePrimitive::SlotArc,
        CorePrimitive::Rectangle,
        CorePrimitive::RoundedRectangle,
        CorePrimitive::RoundedPolygon,
        CorePrimitive::Polygon,
        CorePrimitive::Profile,
        CorePrimitive::MakeFace,
        CorePrimitive::Text,
        CorePrimitive::Svg,
        CorePrimitive::Stl,
    ];
    names.extend(primitives.map(|p| core_operation_name(&CoreOperation::Primitive(p))));

    let booleans = [
        CoreBooleanOp::Union,
        CoreBooleanOp::Difference,
        CoreBooleanOp::Intersection,
        CoreBooleanOp::Xor,
    ];
    names.extend(booleans.map(|b| core_operation_name(&CoreOperation::Boolean(b))));

    let transforms = [
        CoreTransformOp::Translate,
        CoreTransformOp::Rotate,
        CoreTransformOp::Scale,
        CoreTransformOp::Mirror,
    ];
    names.extend(transforms.map(|t| core_operation_name(&CoreOperation::Transform(t))));

    let surfaces = [
        CoreSurfaceOp::Extrude,
        CoreSurfaceOp::Revolve,
        CoreSurfaceOp::Loft,
        CoreSurfaceOp::Sweep,
        CoreSurfaceOp::Shell,
        CoreSurfaceOp::Offset,
        CoreSurfaceOp::OffsetRounded,
        CoreSurfaceOp::Fillet,
        CoreSurfaceOp::Chamfer,
        CoreSurfaceOp::Taper,
        CoreSurfaceOp::Twist,
        CoreSurfaceOp::Draft,
    ];
    names.extend(surfaces.map(|s| core_operation_name(&CoreOperation::Surface(s))));

    let paths = [
        CorePathOp::Polyline,
        CorePathOp::BezierPath,
        CorePathOp::Bspline,
    ];
    names.extend(paths.map(|p| core_operation_name(&CoreOperation::Path(p))));

    let arrays = [
        CoreArrayOp::LinearArray,
        CoreArrayOp::RadialArray,
        CoreArrayOp::GridArray,
        CoreArrayOp::ArcArray,
        CoreArrayOp::Repeat,
        CoreArrayOp::RepeatUnion,
        CoreArrayOp::RepeatCompound,
        CoreArrayOp::RepeatPick,
    ];
    names.extend(arrays.map(|a| core_operation_name(&CoreOperation::Array(a))));

    let frames = [
        CoreFrameOp::Plane,
        CoreFrameOp::Location,
        CoreFrameOp::PathFrame,
        CoreFrameOp::Place,
        CoreFrameOp::ClipBox,
    ];
    names.extend(frames.map(|f| core_operation_name(&CoreOperation::Frame(f))));

    let metas = [CoreMetaOp::Group, CoreMetaOp::Comment, CoreMetaOp::Annotate];
    names.extend(metas.map(|m| core_operation_name(&CoreOperation::Meta(m))));

    names
}

/// Canonical surface name for a non-`Custom` `CoreOperation`.
///
/// Mirrors the private `operation_name` mapping in `ecky_core_ir::signatures`
/// (which cannot be re-exported from this module's write scope). The `match` is
/// exhaustive over `CoreOperation`, so a new variant breaks compilation here —
/// keeping the suggester's vocabulary in lockstep with the registry. `Custom` is
/// unreachable in practice (callers only pass concrete variants) but is handled
/// without panicking to keep this function total and pure.
fn core_operation_name(op: &CoreOperation) -> &'static str {
    match op {
        CoreOperation::Primitive(CorePrimitive::Box) => "box",
        CoreOperation::Primitive(CorePrimitive::Sphere) => "sphere",
        CoreOperation::Primitive(CorePrimitive::Cylinder) => "cylinder",
        CoreOperation::Primitive(CorePrimitive::Cone) => "cone",
        CoreOperation::Primitive(CorePrimitive::Torus) => "torus",
        CoreOperation::Primitive(CorePrimitive::Wedge) => "wedge",
        CoreOperation::Primitive(CorePrimitive::Ellipse) => "ellipse",
        CoreOperation::Primitive(CorePrimitive::Slot) => "slot-overall",
        CoreOperation::Primitive(CorePrimitive::SlotArc) => "slot-arc",
        CoreOperation::Primitive(CorePrimitive::Circle) => "circle",
        CoreOperation::Primitive(CorePrimitive::Rectangle) => "rectangle",
        CoreOperation::Primitive(CorePrimitive::RoundedRectangle) => "rounded-rect",
        CoreOperation::Primitive(CorePrimitive::RoundedPolygon) => "rounded-polygon",
        CoreOperation::Primitive(CorePrimitive::Polygon) => "polygon",
        CoreOperation::Primitive(CorePrimitive::Profile) => "profile",
        CoreOperation::Primitive(CorePrimitive::MakeFace) => "make-face",
        CoreOperation::Primitive(CorePrimitive::Text) => "text",
        CoreOperation::Primitive(CorePrimitive::Svg) => "svg",
        CoreOperation::Primitive(CorePrimitive::Stl) => "import-stl",
        CoreOperation::Boolean(CoreBooleanOp::Union) => "union",
        CoreOperation::Boolean(CoreBooleanOp::Difference) => "difference",
        CoreOperation::Boolean(CoreBooleanOp::Intersection) => "intersection",
        CoreOperation::Boolean(CoreBooleanOp::Xor) => "xor",
        CoreOperation::Transform(CoreTransformOp::Translate) => "translate",
        CoreOperation::Transform(CoreTransformOp::Rotate) => "rotate",
        CoreOperation::Transform(CoreTransformOp::Scale) => "scale",
        CoreOperation::Transform(CoreTransformOp::Mirror) => "mirror",
        CoreOperation::Surface(CoreSurfaceOp::Extrude) => "extrude",
        CoreOperation::Surface(CoreSurfaceOp::Revolve) => "revolve",
        CoreOperation::Surface(CoreSurfaceOp::Loft) => "loft",
        CoreOperation::Surface(CoreSurfaceOp::Sweep) => "sweep",
        CoreOperation::Surface(CoreSurfaceOp::Shell) => "shell",
        CoreOperation::Surface(CoreSurfaceOp::Offset) => "offset",
        CoreOperation::Surface(CoreSurfaceOp::OffsetRounded) => "offset-rounded",
        CoreOperation::Surface(CoreSurfaceOp::Fillet) => "fillet",
        CoreOperation::Surface(CoreSurfaceOp::Chamfer) => "chamfer",
        CoreOperation::Surface(CoreSurfaceOp::Taper) => "taper",
        CoreOperation::Surface(CoreSurfaceOp::Twist) => "twist",
        CoreOperation::Surface(CoreSurfaceOp::Draft) => "draft",
        CoreOperation::Path(CorePathOp::Polyline) => "path",
        CoreOperation::Path(CorePathOp::BezierPath) => "bezier-path",
        CoreOperation::Path(CorePathOp::Bspline) => "bspline",
        CoreOperation::Array(CoreArrayOp::LinearArray) => "linear-array",
        CoreOperation::Array(CoreArrayOp::RadialArray) => "radial-array",
        CoreOperation::Array(CoreArrayOp::GridArray) => "grid-array",
        CoreOperation::Array(CoreArrayOp::ArcArray) => "arc-array",
        CoreOperation::Array(CoreArrayOp::Repeat) => "repeat",
        CoreOperation::Array(CoreArrayOp::RepeatUnion) => "repeat-union",
        CoreOperation::Array(CoreArrayOp::RepeatCompound) => "repeat-compound",
        CoreOperation::Array(CoreArrayOp::RepeatPick) => "repeat-pick",
        CoreOperation::Frame(CoreFrameOp::Plane) => "plane",
        CoreOperation::Frame(CoreFrameOp::Location) => "location",
        CoreOperation::Frame(CoreFrameOp::PathFrame) => "path-frame",
        CoreOperation::Frame(CoreFrameOp::Place) => "place",
        CoreOperation::Frame(CoreFrameOp::ClipBox) => "clip-box",
        CoreOperation::Meta(CoreMetaOp::Group) => "compound",
        CoreOperation::Meta(CoreMetaOp::Comment) => "comment",
        CoreOperation::Meta(CoreMetaOp::Annotate) => "annotate",
        // Open escape hatch — not part of the finite suggestable vocabulary.
        CoreOperation::Custom(_) => "",
    }
}

/// Levenshtein edit distance between two strings, over Unicode scalar values.
///
/// Standard two-row dynamic program: O(a*b) time, O(min) space. Pure, total,
/// no allocation beyond the two rolling rows.
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();

    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }

    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr: Vec<usize> = vec![0; b.len() + 1];

    for (i, &ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            curr[j + 1] = (prev[j + 1] + 1) // deletion
                .min(curr[j] + 1) // insertion
                .min(prev[j] + cost); // substitution
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn near_miss_suggests_box() {
        let suggestions = suggest_ops("bx");
        assert!(
            suggestions.contains(&"box".to_string()),
            "expected `bx` to suggest `box`, got {suggestions:?}"
        );
    }

    #[test]
    fn far_off_garbage_suggests_nothing() {
        assert!(
            suggest_ops("qwertyuiop").is_empty(),
            "expected far-off garbage to produce no suggestions"
        );
    }

    #[test]
    fn exact_op_is_returned_as_its_own_suggestion() {
        // Design choice: an exactly-valid op is its own nearest match (distance 0).
        let suggestions = suggest_ops("box");
        assert_eq!(
            suggestions.first().map(String::as_str),
            Some("box"),
            "expected an exact op to be returned as the closest suggestion, got {suggestions:?}"
        );
    }

    #[test]
    fn suggestions_are_ordered_closest_first() {
        // `unon` is distance 1 from `union`; ensure the closest lands first.
        let suggestions = suggest_ops("unon");
        assert_eq!(
            suggestions.first().map(String::as_str),
            Some("union"),
            "got {suggestions:?}"
        );
    }

    #[test]
    fn suggestions_are_capped() {
        assert!(
            suggest_ops("rotate").len() <= MAX_SUGGESTIONS,
            "suggestions must be capped at {MAX_SUGGESTIONS}"
        );
    }

    #[test]
    fn empty_input_suggests_nothing() {
        assert!(suggest_ops("").is_empty());
        assert!(suggest_ops("   ").is_empty());
    }

    #[test]
    fn known_op_names_are_nonempty_and_include_core_vocabulary() {
        let names = known_op_names();
        assert!(names.contains(&"box"));
        assert!(names.contains(&"union"));
        assert!(names.contains(&"extrude"));
        // The Custom escape hatch must not leak in as an empty name.
        assert!(!names.contains(&""), "Custom op leaked into the name set");
    }

    #[test]
    fn levenshtein_matches_known_values() {
        assert_eq!(levenshtein("box", "box"), 0);
        assert_eq!(levenshtein("bx", "box"), 1);
        assert_eq!(levenshtein("", "box"), 3);
        assert_eq!(levenshtein("kitten", "sitting"), 3);
    }
}
