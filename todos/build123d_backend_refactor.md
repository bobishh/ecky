# Refactor Plan: High-Quality build123d Backend

## Execution Ledger

### Baseline
- `let` in build123d lowerer still smuggles lexical locals through `ParamValue::String`.
- build123d lowering still monolithic and stringly inside `src-tauri/src/ecky_ir.rs`.
- `shell revolve` lowering still boolean-subtracts an inner revolve instead of solid `offset(... openings=planar_faces)`.
- `server/compare_metric.py` still reports bogus STL volume (`0.00`) and cannot gate parity honestly.
- Ecky IR rule stays strict: no mutable `def`, assignment, rebinding, set/update forms. Reject, do not expand.

### Current Tranche
- Mixed tranche, strict order:
  1. track-first
  2. typed internal lowering seam
  3. lexical scope cleanup for `let`
  4. sketch/path/solid kind checks
  5. exact `shell revolve` semantics
  6. canonical fixture pair + native-vs-lowered parity harness
  7. module split for build123d lowering
  8. stale fixture cleanup
- Steel front-end landed; next tranche deletes the legacy string bridge and uses an in-memory CoreIR -> backend adapter.

### Next Tasks
- [x] Introduce typed build123d lowering scope/bindings.
- [x] Remove build123d `let` hack through `ParamValue::String`; resolve locals lexically.
- [x] Convert sketch/path/solid hotspots and `intersect-x` to typed resolvers.
- [x] Switch build123d `shell revolve` to solid `offset(... openings=planar_faces)`.
- [x] Refresh canonical cup fixtures and repair STL volume compare path.
- [x] Move build123d lowering into `src-tauri/src/build123d_lowering.rs` and keep `lower_to_build123d(source)` stable.
- [x] Add canonical parity fixture pair under `src-tauri/tests/fixtures/cad/`.
- [x] Add one-shot parity harness `python3 server/check_canonical_cup_parity.py`.
- [x] Stop using stale `ref_cup.stl` / `ref.stl` as acceptance truth.
- [x] Delete stale parity helper files and references from active workflow.

### Verification
- `cd src-tauri && cargo test lower_to_build123d --lib` -> pass (`59 passed`).
- `cd src-tauri && cargo test` -> pass (`367 passed`).
- `cd src-tauri && cargo check` -> pass.
- `python3 server/test_lower.py` -> `Canonical cup fixture OK`.
- `python3 server/check_canonical_cup_parity.py` -> `EXCELLENT MATCH`.
- Native reference volume `127692.36 mm^3`; lowered/generated volume `127947.14 mm^3`.
- Bounding-box deltas `dx=0.00 dy=0.00 dz=0.00`.

### Open Risks
- Giant `match` still exists inside lowerer logic, but build123d lowering now lives in `src-tauri/src/build123d_lowering.rs` instead of `ecky_ir.rs`.
- build123d transform/boolean ops can leak wrong kind unless every branch preserves or validates kind.
- build123d-specific tests still live in `ecky_ir.rs` test module. Behavior fine; further test-file split still optional cleanup.

## Objective
Transition the build123d lowerer from a set of hardcoded node handlers to a robust, architectural translation engine that accurately maps Ecky IR v0 to idiomatic build123d Python code.

## Goals
1. **Architectural Purity**:
    - Implement a generic keyword/property parser (`:key value`) to avoid `if/while` mess in node handlers.
    - Standardize variable resolution for symbols defined by lexical `let` blocks only.
    - **Non-negotiable IR rule:** Ecky IR v0 stays declarative and immutable. Do not add Python/Lisp-style mutable `def`, assignment, rebinding, set/update forms, or hidden statement ordering to the IR. If existing code accepts `(def ...)`, treat that as technical debt to remove or reject with a validation error, not as a feature to expand.
2. **Geometric Correctness**:
    - Ensure all 2D profiles (`bspline`, `polygon`, `circle`) are correctly closed and converted to `Face` objects before being used in 3D operations.
    - Guarantee `Solid` output (volume > 0) for operations like `revolve` and `extrude`.
3. **Feature Parity**:
    - Full support for `build123d` specifics: `tangents`, `tangent_scalars`, `fillet`, `chamfer`, and `offset` (including `openings`).
4. **Automated Validation**:
    - Utilize the `compare_models` MCP tool to verify generated geometry against industry-standard references (e.g., the `build123d` documentation cup).

## Current Status (2026-04-17)
- [x] Fixed primitive handling: support exists for integers, symbols, and nested point lists.
- [x] Improved `revolve`: current build123d lowerer rotates sketches into XZ before `revolve(... axis=Axis.Z ...)`.
- [x] Implemented `compare_models` MCP: tool exists and compares STL volume/bounding boxes.
- [x] Canonical truth moved from stale STL blobs to native build123d code in `src-tauri/tests/fixtures/cad/surface/canonical_cup.build123d.py`.
- [x] Canonical authored fixture now lives at `src-tauri/tests/fixtures/cad/surface/canonical_cup.ecky`.
- [x] One-shot parity harness exists: `python3 server/check_canonical_cup_parity.py`.
- [x] Native build123d STL vs lowered Ecky IR STL now reports `EXCELLENT MATCH`.
- [x] Added broad lowerer coverage: unit tests exist for extrude, difference, params, numeric expressions, transforms, shell variants, revolve, loft, sweep, arrays, fillet/chamfer, bspline, text/svg/import-stl.
- [x] Generic keyword parsing now feeds `bspline`, `offset :openings`, and edge selectors without node-local manual keyword loops.
- [x] Solid-first lowering now coerces `extrude`, `revolve`, `shell extrude`, and `shell revolve` sketches through `_ecky_face(...)` before 3D operations.
- [x] IR immutability enforced in the build123d lowerer: `(def ...)` now hard-rejects instead of emitting Python assignment.
- [x] Agent-facing docs/tool schema now warn that Ecky IR is immutable and must not use `(def ...)`, assignment, rebinding, set/update forms, or Python helpers.
- [x] Docs/code drift fixed for `rounded-polygon` and `bspline` in the build123d prompt guide.
- [x] `compare_models` script resolution fixed to use repo fallbacks when bundled resource lookup points at a missing `target/debug/server/compare_metric.py`.
- [x] build123d lowering extracted into `src-tauri/src/build123d_lowering.rs`; `ecky_ir.rs` keeps AST/shared IR ownership.

## Implementation Steps
1. **Generic Property Parsing**: 
    - [x] Refactor `lower_geometry` to extract all `:keywords` before matching the node.
    - [x] Create helpers to map IR keywords to build123d Python arguments (e.g., `:tangents` -> `tangents=`).
2. **Solid-First Pipeline**:
    - [x] Update `revolve` and `extrude` handlers to automatically wrap input edges in `Wire` and `make_face` if they aren't already faces.
3. **Reference Verification**:
    - [x] Compare lowered Ecky IR cup STL against native build123d cup STL.
    - [x] Achieve an "EXCELLENT MATCH" status via `python3 server/check_canonical_cup_parity.py`.
4. **Cleanup**:
    - [x] Remove debug `println!`/`dbg!` from the build123d lowering path.
