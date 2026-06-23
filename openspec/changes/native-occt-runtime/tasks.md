# Tasks: Native OCCT Runtime

## Worker Rules

- Use cheap implementation workers for T1-T5.
- Each worker edits only its assigned write scope.
- Workers must not remove build123d/OCP/FreeCAD/Python CAD runtime paths.
- Workers must not revert unrelated edits.
- Workers must list changed files and tests run.
- Main thread reviews before integration.

## 1. T1 - Standalone OCCT SDK Probe

Write scope:

- `src-tauri/src/ecky_cad_host/direct_occt_sdk.rs`
- local tests in same module

Tasks:

- [x] 1.1 Add SDK manifest data model for `runtime/occt/manifest.json`.
- [x] 1.2 Add `ECKY_OCCT_ROOT` probe path before existing runtime discovery.
- [x] 1.3 Add bundled `runtime/occt` resource probe path.
- [x] 1.4 Support platform-specific library extensions: macOS `.dylib`, Linux
  `.so`, Windows `.dll` / import library handling.
- [x] 1.5 Report raw structured blockers for missing manifest fields, headers,
  libraries, platform mismatch, and ABI mismatch.
- [x] 1.6 Add unit tests for valid manifest, missing lib, wrong platform, and
  fallback to current runtime probe.

## 2. T2 - Native Runner ABI

Write scope:

- `docs/direct-occt-runner-abi.md`
- optional isolated prototype only if explicitly authorized

Tasks:

- [x] 2.1 Specify `plan.json` schema version and top-level fields.
- [x] 2.2 Specify runner CLI shape:
  `direct-occt-runner --plan plan.json --out <bundle-dir>`.
- [x] 2.3 Specify outputs: `model.step`, `preview.stl`, `topology.json`,
  stdout, stderr, exit code.
- [x] 2.4 Specify native runner error classes.
- [x] 2.5 Specify compatibility between Rust `OcctPlan` and runner schema.
- [x] 2.6 Add migration path from generated C++ source to precompiled runner.

## 3. T3 - Core Normalizer

Write scope:

- new `src-tauri/src/ecky_cad_host/direct_occt_normalize.rs`
- `src-tauri/src/ecky_cad_host/mod.rs`
- targeted tests

Tasks:

- [x] 3.1 Add normalizer entrypoint:
  `normalize_core_program_for_direct_occt(program, params)`.
- [x] 3.2 Resolve scalar `if` branches where conditions are evaluable.
- [x] 3.3 Expand finite `range`.
- [x] 3.4 Expand finite `map`.
- [x] 3.5 Expand `apply` over finite lists.
- [x] 3.6 Expand `repeat`, `repeat-union`, `repeat-compound`, `repeat-pick`.
- [x] 3.7 Preserve existing `sampled-radial-loft` and shell expansion behavior.
- [x] 3.8 Reject `xor`, unfilled `hole`, unsupported custom ops, and mesh-only
  ops before native compilation.
- [x] 3.9 Add tests for happy path and deterministic rejection paths.

## 4. T4 - SVG Profile Ingestion

Write scope:

- new `src-tauri/src/ecky_cad_host/svg_profile.rs`
- `src-tauri/Cargo.toml`
- targeted tests

Tasks:

- [x] 4.1 Add Rust SVG parser dependency, preferably `usvg`.
- [x] 4.2 Define profile loop data model: outer loop, hole loops, fit metadata.
- [x] 4.3 Parse SVG viewBox, units, transforms, and visible paths.
- [x] 4.4 Convert closed vector paths into deterministic loop coordinates.
- [x] 4.5 Classify holes by containment/orientation.
- [x] 4.6 Implement fit modes: contain, cover, stretch.
- [x] 4.7 Reject raster-only SVG, open contours, self-intersections,
  near-zero-area loops, and multi-outer-loop input for first slice.
- [x] 4.8 Add tests for outer loop, hole loop, viewBox fit, invalid raster,
  open path, and self-intersection.

## 5. T5 - Topology and Selector Proof

Write scope:

- tests in `src-tauri/src/ecky_cad_host/direct_occt_runtime.rs`
- tests in `src-tauri/src/ecky_cad_host/direct_occt_executor.rs`
- production edits only if isolated and required

Tasks:

- [x] 5.1 Add test proving direct OCCT STEP/STL/topology bundle output remains
  complete.
- [x] 5.2 Add edit-cycle test for stable/durable face target aliases.
- [x] 5.3 Add edit-cycle test for stable/durable edge target aliases.
- [x] 5.4 Add test showing direct OCCT selection targets remain intentionally
  non-editable until exact control binding exists.
- [x] 5.5 Add failure test for malformed or missing `topology.json`.
- [x] 5.6 Document broad selector filtering requirements if not implemented.

## 6. T6 - Product Integration and Gates

Write scope:

- integration edits only after T1-T5 review
- docs/tracker updates allowed

Tasks:

- [x] 6.1 Review worker outputs and merge only non-conflicting patches.
- [x] 6.2 Run `cd src-tauri && cargo check`.
- [x] 6.3 Run relevant Rust tests for direct OCCT modules.
- [x] 6.4 Run frontend/unit tests only if UI/export status changes.
- [x] 6.5 Evaluate Playwright gate: not run because user-visible UI flow did
  not change.
- [x] 6.6 Update OpenSpec tasks as each task completes.
- [x] 6.7 Do not remove dependencies until proof gates all pass.

## 7. T7 - Vendored OCCT and Unified Target Order

Write scope:

- `scripts/prepare_occt_runtime.sh`
- `package.json`
- `src-tauri/tauri.conf.json`
- `src-tauri/src/runtime_capabilities.rs`
- docs/tracker updates

Tasks:

- [x] 7.1 Add `occt:prepare` script that builds `.dist/runtime/occt`.
- [x] 7.2 Add Tauri resource mapping for `runtime/occt`.
- [x] 7.3 Prefer `ECKY_OCCT_ROOT` / bundled `runtime/occt` before build123d
  runtime discovery.
- [x] 7.4 Smoke vendored `.dist/runtime/occt` with direct OCCT export.
- [x] 7.5 Document unified target order: Core IR classification, Direct OCCT
  first for supported BRep, Rust mesh fallback, lithophane post-processing last.
- [x] 7.6 Keep dependency removal out of this change; package proof only.

## 8. T8 - Core IR Coverage Matrix

Write scope:

- `docs/direct-occt-coverage-matrix.md`
- tests only when adding missing coverage proof

Tasks:

- [x] 8.1 Inventory every Core IR primitive, boolean, transform, surface, path,
  array, frame, meta, and custom/special operation.
- [x] 8.2 Classify each operation as direct-OCCT supported, Rust mesh-only,
  exact fallback, explicit unsupported, or unknown.
- [x] 8.3 Link each supported operation to at least one planner test or live
  export test.
- [x] 8.4 Link each rejected operation to a deterministic error test.
- [x] 8.5 Identify missing tests and exact fallback dependencies that block
  build123d fallback removal.

## 9. T9 - Exact Ops Closure

Write scope:

- `src-tauri/src/ecky_cad_host/direct_occt.rs`
- `src-tauri/src/ecky_cad_host/direct_occt_executor.rs`
- `src-tauri/src/ecky_cad_host/direct_occt_normalize.rs`
- targeted tests
- coverage matrix updates

Tasks:

- [x] 9.1 Convert every currently exact-fallback operation to direct OCCT, or
  mark it explicit unsupported with raw actionable diagnostics.
- [x] 9.2 Add live export tests for newly covered exact ops.
  - `sampled-radial-loft` and shell/dome stacks already have Direct OCCT live
    export tests; no new exact op was converted in this pass.
- [x] 9.3 Add deterministic rejection tests for exact ops not worth supporting.
- [x] 9.4 Remove silent exact fallback for operations classified unsupported.
- [x] 9.5 Keep Rust mesh/lithophane post-processing behavior unchanged.

## 10. T10 - Precompiled Direct OCCT Runner

Write scope:

- `src-tauri/src/ecky_cad_host/direct_occt_runner.rs`
- `src-tauri/src/ecky_cad_host/direct_occt_executor.rs`
- `src-tauri/src/ecky_cad_host/direct_occt_sdk.rs`
- runner source/build scripts as needed
- targeted tests

Tasks:

- [x] 10.1 Serialize resolved runner-safe `OcctPlan` subset to `plan.json` matching
  `docs/direct-occt-runner-abi.md`.
- [x] 10.2 Add runner executable discovery beside SDK discovery.
- [x] 10.3 Implement runner invocation:
  `direct-occt-runner --plan plan.json --out <bundle-dir>`.
- [x] 10.4 Preserve stdout/stderr/exit-code raw error details.
- [x] 10.5 Add runner smoke/parity proof for the supported starter fixture.
- [x] 10.6 Keep generated-source path available as fallback until parity gates
  pass.
- [x] 10.7 Add live runner proof for expanded keyword-free Direct OCCT ops:
  transforms, arrays, frames, profiles, paths, loft/sweep/taper/twist/revolve/
  offset.
- [x] 10.7a Replace the hand-rolled runner JSON parser with vendored `yyjson`
  and direct typed decode.
- [x] 10.8 Add runner keyword/selector parity proof before routing
  selector-driven ops through the runner. CLOSED 2026-07-06: all sub-items
  10.8a-10.8h are done (10.8h verified this session, 10.8a-g pre-existing).
- [x] 10.8a Parse runner `keywords` natively and reject unsupported keyword
  shapes with deterministic schema/validation errors.
- [x] 10.8b Emit runner topology `targetId` values for exact selector replay
  and parity with generated-source topology contracts.
- [x] 10.8c Add native runner support plus runner-first host gate coverage for
  `profile :holes`.
- [x] 10.8d Add native runner support plus runner-first host gate coverage for
  `clip-box`.
- [x] 10.8e Add exact edge-target selector support for runner `fillet` and
  `chamfer`.
- [x] 10.8f Add exact face-target selector support for runner `shell`.
- [x] 10.8g Add clause-filter selector support for runner `shell`.
- [x] 10.8h Add clause-filter selector support for runner `fillet` and
  `chamfer`. VERIFIED 2026-07-06 (already implemented, checkbox was stale):
  `runner_exact_edge_selector_supported` accepts `EdgeClauses` (line 615,
  shared with 10.8e's exact-id path); C++ `fillet_shape`/`chamfer_shape`
  resolve `SelectorPayloadType::Clauses` via `resolve_edge_clauses`; live
  proof `live_precompiled_runner_accepts_exact_selector_plans_when_available`
  exercises `clause_fillet_plan`/`clause_chamfer_plan` through the real
  runner binary — reran green.
- [ ] 10.9 Add generated-source vs runner parity fixtures for every remaining
  Direct OCCT op before removing generated-source fallback. PROGRESS
  2026-07-06: `draft` was the only `OcctOp` variant with zero runner dispatch
  (generated-source-only, C++ `BRepOffsetAPI_DraftAngle` side-wall-face
  algorithm existed only in `direct_occt_executor.rs`'s emitted-source path).
  Closed: added `draft_shape` to `direct_occt_runner.cpp` (mirrors
  `emit_draft_operation`'s neutral-plane/pull-direction logic exactly),
  `runner_draft_keywords_supported` + `OcctOp::Draft` in
  `runner_op_supported`/`runner_command_supported`
  (`direct_occt_runner.rs`), rebuilt+synced the runner binary. Proof: routing
  gate test, live runner-execution proof (`draft_plan`/`draft_neutral_z_plan`
  in `live_precompiled_runner_accepts_exact_selector_plans_when_available`),
  and a real geometric parity test against build123d
  (`live_draft_matches_build123d_reference`, using the [[5.1 harness]]).
  REMAINING: the rest of the runner-supported ops (box/sphere/.../compound)
  have build123d-differential coverage and "faces non-empty" runner-execution
  proofs, but not a systematic generated-source-vs-runner A/B fixture matrix
  per op — that full matrix is still open.

## 11. T11 - Switch Direct OCCT Export Path To Runner

Write scope:

- `src-tauri/src/ecky_cad_host/direct_occt_executor.rs`
- `src-tauri/src/ecky_cad_host/direct_occt_runtime.rs`
- `src-tauri/src/services/render.rs`
- targeted tests

Tasks:

- [x] 11.1 Add runtime flag/config path for runner-first vs generated-source
  fallback.
- [x] 11.2 Route direct OCCT exports through runner first when runner exists
  and the plan fits the supported keyword-free runner subset.
- [x] 11.3 Fall back to generated-source path on missing runner, explicit
  runner-disabled mode, or runner-unsupported plan shape.
- [x] 11.4 Add regression test proving STEP/STL/topology output stays identical
  at artifact-contract level.
- [x] 11.5 Update docs to state generated C++ compile remains fallback for
  unsupported/keyword plans until full runner parity is green.
- [x] 11.6 Expand runner-first default to the proven keyword-free runner subset,
  including frame ops.
- [x] 11.7 Expand runner-first default to keyword/selector/exact plans only
  after native runner implementation and parity proof. VERIFIED 2026-07-06:
  production entry point
  `direct_occt_runtime::render_core_program_runtime_bundle_with_font_path`
  (called from `services/render.rs`) →
  `direct_occt_executor::export_core_program_step_stl_with_params_runner_first`
  → `direct_occt_runner::run_plan_step_stl_with_mode`, which gates solely on
  `runner_supports_plan(plan)` — the same full gate proven by 10.8a-h
  (target-id/clause/edge-all selectors for fillet/chamfer/shell, profile
  holes, clip-box, plane/path-frame keywords, bspline, sweep frenet, now
  draft). No separate keyword-free-only gate exists in the production path;
  `runner_enabled()` is only an on/off kill-switch
  (`ECKY_DIRECT_OCCT_RUNNER_DISABLED`), not a scope limiter. Default already
  is exactly "keyword/selector/exact plans, once proven" as specified.

## Proof Gates

- [x] G1 Native OCCT SDK path exports STEP/STL/topology without OCP Python
  package.
- [x] G2 Current direct OCCT fixtures render.
- [x] G3 One generated parameterized model renders through native path.
- [x] G4 SVG profile extrudes into valid STEP.
- [x] G5 Topology IDs survive one edit cycle.
- [x] G6 Mesh/lithophane Rust paths still work.
- [x] G7 Raw native errors reach UI.
- [x] G8 `cd src-tauri && cargo check` passes.
- [x] G9 Coverage matrix covers every Core IR operation.
- [x] G10 Exact fallback is eliminated or replaced by explicit unsupported
  diagnostics.
- [x] G11 Precompiled runner exports STEP/STL/topology from `plan.json`.
- [x] G12 Runner-first path replaces per-render generated C++ compile by
  default for runner-supported plans; generated C++ stays fallback for
  unsupported/keyword plans.
- [ ] G13 Full Direct OCCT op set has generated-source vs runner parity proof,
  including path/array/surface/selector-driven plans. Same remaining scope as
  10.9: the runner-supported subset works and is proven live, but not every
  op has a systematic generated-source-vs-runner A/B fixture.
- [x] G14 Runner keyword/selector plans execute without generated-source
  fallback. VERIFIED 2026-07-06 (see 11.7): re-ran the 12 live
  `live_direct_occt_runtime_applies_*_when_{sdk,runner}_ready` tests in
  `direct_occt_runtime.rs` — each asserts "should not emit generated C++
  source" for exact/coarse/edge-all edge selectors, shell clause/exact/
  keywordless selectors, on both fillet and chamfer. All 12 green.
