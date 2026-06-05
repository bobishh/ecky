# Design: Verify Distance Metrics

## Goal

Add one deterministic distance metric family that is strict about selector
resolution and uses existing runtime evidence.

## Decisions

- Metric contract is explicit:
  - `clearance` namespace
  - `min-distance` metric key
  - two selector operands required
- Selector operands are authored names, not free-form code.
- Selector resolution order is deterministic and bounded.
- Existing `StructuralVerificationResult` remains the carrier.
- Distance failures are additive authored verify issues, not a new contract.

## Metric Shape

```clojure
(verify
  (tag front_gap body.front_window_1)
  (metric gap (clearance min-distance body.front_window_1 lid.front_skirt))
  (expect gap (>= 3)))
```

Selector resolution accepts names that resolve to:

- `PartBinding.partId`
- `SelectionTarget.targetId`, `durableTargetId`, `canonicalTargetId`, or any alias
- `ViewerEdgeTarget.targetId`, `durableTargetId`, `canonicalTargetId`, or any alias
- `ViewerFaceTarget.targetId`, `durableTargetId`, `canonicalTargetId`, or any alias
- `FeatureOutputRef` as `featureId.outputId`, then mapped through correspondence edges when needed

## Geometry Evidence

- Part selectors use manifest `bounds` and AABB distance.
- Edge selectors use edge segment endpoints.
- Face selectors use face center points.
- Output selectors resolve through correspondence graph to target ids before geometry lookup.

If selectors resolve to multiple pieces of evidence, use the minimum pairwise
distance across resolved anchors.

## Rejected Paths

- Reusing `manifest`/`stl` as a vague distance namespace. Rejected because it
  keeps the contract misleading.
- Adding a general selector language now. Rejected because the first useful
  slice only needs named selectors.
- Introducing a new public result structure. Rejected because the structural
  issue carrier already exists.

## Proof Plan

- Rust unit tests for selector resolution and min-distance math.
- Rust integration coverage for authored verify merge with pass/fail/error.
- e2e proof for the docs/workbench path that can write and run the new metric.
