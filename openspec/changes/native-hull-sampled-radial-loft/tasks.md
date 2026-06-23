# Tasks: Native hull + sampled-radial-loft

## 1. sampled-radial-loft native (red → green)

- [x] 1.1 Red: differential parity test — EckyRust render of a
      sampled-radial-loft macro vs build123d reference
      (`live_differential_sampled_radial_loft_matches_build123d`).
- [x] 1.2 Remove `sampled-radial-loft` from `EXACT_BACKEND_ONLY_CAD_OPS`
      (const removed; folded into `CAD_OPS_PORTABLE`); present in all three
      backend manifests; support notes refreshed.
- [x] 1.3 Update render gates + tests that asserted the old rejection
      (removed exact-only detection from `ecky_ir/mod.rs`, mesh-mix
      diagnostic, `source_uses_exact_backend_only_cad_ops` + its ir tests;
      reframed the two `does_not_silently_build123d_fallback` tests to the
      Direct-OCCT "No mesh fallback" diagnostic).
- [x] 1.4 Green: 16 sampled-radial-loft tests pass incl. differential parity
      on the runner tier.

## 2. hull op (red → green)

- [x] 2.1 Red: compile/plan test for `(hull ...)`
      (`plans_hull_for_direct_occt`).
- [x] 2.2 Language surface: `ECKY_RUST_DIRECT_ONLY_CAD_OPS`, cad module
      export, reference entry with native-only support string.
- [x] 2.3 Core IR: `hull` routes as Direct-OCCT-required; build123d/FreeCAD
      lowerings reject with a diagnostic naming the op
      (`lower_to_build123d_rejects_hull`, `freecad_lowering_rejects_hull`).
- [x] 2.4 Planner: `OcctOp::Hull` emission with variadic shape refs.
- [x] 2.5 Runner C++: tessellated point cloud + BREP vertices → incremental
      3-D hull → sew shell → solid (`convex_hull_shapes`); runner gate
      admits `hull`; runner rebuilt.
- [x] 2.6 Generated-source fallback: same algorithm emitted as
      `ecky_convex_hull_shapes` helper (shim-tier parity proven with
      `ECKY_DIRECT_OCCT_RUNNER_DISABLED=1`).
- [x] 2.7 Green: `live_executor_exports_hull_capsule_with_analytic_volume`
      (volume within 3 % of analytic capsule and never above it, exact bbox,
      1 component, 0 non-manifold edges) + `ecky_rust_request_renders_hull_natively`.

## 3. Docs + closure

- [x] 3.1 Update `docs/direct-occt-coverage-matrix.md` (hull rows,
      sampled-radial-loft reclassification, stale helical-ridge rows).
- [x] 3.2 Full backend suite green: 1552 passed / 0 failed / 2 ignored
      (`RUST_MIN_STACK=67108864 cargo test --lib`). Also repaired the
      pre-existing red the run surfaced: db test schema drift
      (`structural_verification`), bundled-python resolution for parity
      tests, selector-shape/destructuring/guide-content/test drift, digest
      lock regeneration, macro_preview_render diagnostics.
- [x] 3.3 `openspec validate native-hull-sampled-radial-loft`.
