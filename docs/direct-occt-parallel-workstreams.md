# Direct OCCT Parallel Workstreams

Status: active tracking doc.
Rule: proof first, removal later. Cheap implementation workers, disjoint write
scopes.

## Objective

Build native OpenCascade rendering path from existing Ecky Core IR while current
working paths remain intact.

## Workstreams

| ID | Track | Status | Owner | Write Scope | Output |
| --- | --- | --- | --- | --- | --- |
| T1 | OCCT SDK packaging | verified | Sagan + local | `src-tauri/src/ecky_cad_host/direct_occt_sdk.rs`, local tests | `ECKY_OCCT_ROOT` / `runtime/occt` probe beside current probe |
| T2 | Native runner ABI | done | Fermat | new `docs/direct-occt-runner-abi.md` | precompiled runner ABI design, no render-path edits |
| T3 | Core normalizer | verified | Parfit + local | new `src-tauri/src/ecky_cad_host/direct_occt_normalize.rs`, `src-tauri/src/ecky_cad_host/mod.rs`, tests | finite normalizer for `repeat*` and dynamic list skeleton |
| T4 | SVG profile ingestion | verified | Descartes + local | new `src-tauri/src/ecky_cad_host/svg_profile.rs`, `src-tauri/Cargo.toml`, tests | SVG vector paths to profile-loop data |
| T5 | Topology/selectors | verified | Peirce + local | tests in `direct_occt_runtime.rs` / `direct_occt_executor.rs`; isolated helper fix | edit-cycle/topology selector proof tests |
| T6 | Product integration gates | active | local | tracker/docs/tests | integration checklist, review, merge decisions |

## Worker Policy

- Use cheap models: `gpt-5.3-codex-spark` or `gpt-5.4-mini`.
- Use `worker`, not `explorer`, for implementation tracks.
- Each worker edits only its assigned write scope.
- Workers run in forked workspaces.
- Workers must not revert unrelated changes.
- Workers must list changed files and tests run.
- Main thread reviews and integrates; no blind merge.
- No dependency removal in worker tasks.
- No worker starts until explicitly authorized.

## Acceptance Gates

No dependency removal until:

- native OCCT SDK path exports STEP/STL/topology without OCP Python package.
- direct OCCT renders current fixtures.
- one generated model with parameters renders through native path.
- SVG profile extrudes into valid STEP.
- topology IDs survive one edit cycle.
- mesh/lithophane Rust paths still pass.
- raw native errors reach UI.
- `cd src-tauri && cargo check` passes.

## Current Measurements

Observed local sizes:

| Path | Size |
| --- | ---: |
| `.dist/build123d-runtime` | 1.6G |
| `src-tauri/target/release/runtime/build123d` | 1.6G |
| `.dist/build123d-runtime/lib/python3.12/site-packages/OCP` | 254M |
| `.dist/build123d-runtime/lib/python3.12/site-packages/OCP/.dylibs` | 81M |
| `.dist/build123d-runtime/lib/python3.12/site-packages/vtkmodules` | 329M |
| `.dist/build123d-runtime/lib/python3.12/site-packages/tree_sitter_language_pack` | 351M |
| `.dist/runtime/occt` | 71M |
| `src-tauri/target/release/ecky_cad` | 41M |

### G1 historical blocker (standalone native OCCT)

- Expected manifest: `.dist/runtime/occt/manifest.json`
- Expected fields: `schemaVersion`, `platform`, `arch`, `occtVersion`, `abiTag`, `includeDir`, `libDir`, `requiredHeaders`, `requiredLibraries`, `libraryHashes`
- Previous state on this machine: `.dist/runtime/occt` directory was not present.
- Current state: `.dist/runtime/occt` exists and contains a passing manifest plus copied OCCT runtime files.
- Exact blocker when manifest is present but incomplete: `manifestMissingFields` listing the required fields above.

### G1 proof (external standalone OCCT)

- Installed proof SDK: Homebrew `opencascade` `7.9.3`.
- Installed size: `147.3MB`; dependencies installed with it: `tbb` `2.9MB`, `hwloc` `11.3MB`.
- Proof manifest root: `/tmp/ecky-occt-sdk-proof/runtime/occt/manifest.json`.
- Manifest points to `/opt/homebrew/opt/opencascade/include/opencascade` and `/opt/homebrew/opt/opencascade/lib`.
- This proof does not use `.dist/build123d-runtime/lib/python3.12/site-packages/OCP`.
- Passing proof command:
  `ECKY_OCCT_ROOT=/tmp/ecky-occt-sdk-proof cargo test live_executor_exports_core_ir_box_when_runtime_ready -- --nocapture`.
- Repo now has a generated vendored `.dist/runtime/occt` package; dependency removal still waits for a separate removal change.

### G1 proof (vendored OCCT)

- Prepared runtime: `.dist/runtime/occt`.
- Prepared size: `71M`.
- Prepared from Homebrew `opencascade` `7.9.3` with copied OCCT dylibs,
  required headers, manifest, and copied non-system dylib dependencies.
- Tauri resource mapping: `.dist/runtime/occt` -> `runtime/occt`.
- Passing proof command:
  `cargo test live_executor_exports_core_ir_box_when_runtime_ready -- --nocapture`.

### Unified target order

This is one target pipeline, not separate mesh/lithophane and BREP products.

1. Classify Ecky Core IR before dispatch:
   - mesh-only operations force `EckyRust`.
   - exact-only operations stay on requested exact backend.
   - mixed mesh-only plus exact-only input is rejected.
2. For `EckyRust`, try Direct OCCT first when the normalized Core IR is
   supported:
   - success writes `generated-direct-occt-*` bundle ids with STEP, STL, and
     topology targets.
   - failure falls back by operation class.
3. Fallback order:
   - exact-only unsupported by Direct OCCT may use current build123d burn-in
     path while dependencies remain.
   - mesh-only/non-exact Rust path writes `generated-ir-*` mesh output.
4. Post-processing runs after geometry export:
   - lithophane attachments and displacement operate on the final preview STL.
   - target part selection uses the bundle manifest from whichever geometry path
     succeeded.
5. Removal gate:
   - build123d/FreeCAD removal waits until exact fallback is either covered by
     Direct OCCT or explicitly unsupported with raw error details.

## Agent Log

| Agent | Track | Status | Notes |
| --- | --- | --- | --- |
| Russell | T1/T2 | research done | SDK tied to build123d/OCP layout; runner ABI not separated |
| Raman | T3 | research done | planner rejects `If/Range/Map/Apply/repeat*`; normalizer needed |
| Lorentz | T4 | research done | SVG uses FreeCAD/importSVG or build123d/import_svg; no Rust parser |
| Turing | T5/T6 | research done | topology rich; editability and selector stability need gates |
| Sagan | T1 | verified | Added manifest/env/bundled OCCT probe and cross-platform library matching; `cargo test direct_occt_sdk -- --nocapture` passed |
| Fermat | T2 | done | Wrote `docs/direct-occt-runner-abi.md` |
| Parfit | T3 | integrated after stale shutdown | Added normalizer; local fixes made compile/tests pass |
| Descartes | T4 | integrated after stale shutdown | Added `usvg` SVG profile parser; local fixes made compile/tests pass |
| Peirce | T5 | verified | Added topology/selector tests; local C++ durable-stable helper fix made live tests pass |
| Ptolemy | G4 | stale closed, integrated locally | Added SVG primitive expansion to direct OCCT profile plans; local live export proof added |
| Godel | G1 | stale closed, blocker documented | Standalone `.dist/runtime/occt` blocker documented; vendored runtime later prepared locally |
| Rawls | G6 | stale closed, verified locally | Mesh/lithophane Rust path tests passed |
