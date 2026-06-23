# Proposal: Native `hull` Op + Native `sampled-radial-loft`

## Intent

Close two native-coverage gaps that block organic-shape authoring (lasts,
clogs, fairings) on the EckyRust/Direct OCCT path:

1. `sampled-radial-loft` is still classified `EXACT_BACKEND_ONLY_CAD_OPS`
   even though the Direct OCCT planner already expands it into a plain
   `loft` of translated polygon sections. The classification — not the
   geometry — is the blocker: EckyRust requests are rejected, agent
   guidance says "use FreeCAD/build123d", and the language manifest hides
   the op from the native backend.
2. There is no `hull` operation anywhere in the language. Convex hull over
   child shapes (OpenSCAD semantics) is the cheapest way to express blended
   organic bridges between primitives without hand-lofting sections.

## Problem (evidence)

- `docs/direct-occt-coverage-matrix.md` row `sampled-radial-loft`:
  "normalized-direct … exact-only but already covered" — the matrix itself
  records the contradiction.
- `render.rs` test
  `ecky_rust_request_does_not_silently_build123d_fallback_for_sampled_radial_loft`
  asserts a rejection that only exists because of the stale classification.
- `hull` appears in no spec, no change, no source. Slipper/last-style models
  currently require dense manual `loft` stacks for shapes a 2-line hull
  would express.

## Decision

- **`sampled-radial-loft`:** drop it from `EXACT_BACKEND_ONLY_CAD_OPS`;
  it becomes a regular tri-backend op (native expansion already proven by
  planner tests; build123d/FreeCAD lowerings already exist). Native support
  is proven by a differential parity test against an exact reference.
- **`hull`:** new CAD op `(hull shape...)`, native-only (Direct OCCT
  required, like `helical-ridge`): vertices are gathered from tessellated
  child shapes, a 3-D quickhull builds the convex polyhedron, sewn into a
  BREP solid. Exact backends reject `hull` with a deterministic diagnostic
  (no silent fallback), matching the established native-required pattern.
- Runner: `hull` is implemented in the precompiled runner
  (`direct_occt_runner.cpp`) and admitted by the runner gate; the generated
  C++ fallback emits the same helper so both Direct OCCT tiers agree.

## Rejected Paths

- **Implementing `hull` via existing ops (expansion), like
  `sampled-radial-loft`.** Convex hull requires geometry inspection of
  evaluated shapes; the Rust planner never sees evaluated BREP, so no
  expansion into existing plan ops can express it.
- **Exact-backend `hull` implementations.** Neither build123d nor FreeCAD
  Part expose a convex hull; shipping Python-side quickhull triples the
  surface for no user demand. Native-only with explicit rejection keeps the
  contract honest.
- **Mesh-path `hull`.** The mesh renderer is a legacy tier; new ops target
  BREP.

## Scope

- Reclassify `sampled-radial-loft` (language surface, render gates, tests,
  matrix).
- `hull`: language surface entry, scheme→core compile, `OcctOp::Hull`,
  planner emission, runner gate admission, runner C++ quickhull + sewing,
  generated-source emission, exact-lowering rejections, live + differential
  tests, matrix rows.

## Out of Scope

- 2-D hull of sketches (hull accepts solids/shapes; sketch support can
  follow demand).
- `xor`, `text`, `import-stl` native coverage (tracked elsewhere).

## Proof Plan

- Red: differential parity test rendering a `sampled-radial-loft` macro
  natively fails at dispatch (exact-only rejection) before the change.
- Red: `(hull (sphere ...) (translate ... (sphere ...)))` fails to compile /
  plan before the change.
- Green: native sampled-radial-loft render matches the exact reference
  within the established parity tolerances; hull live test produces a
  manifold solid whose volume exceeds the summed inputs and matches the
  analytic spherocylinder envelope within tolerance.
