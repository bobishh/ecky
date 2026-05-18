# Direct OCCT Core Architecture

Status: target architecture.
Goal: render normalized Ecky Core IR through OpenCascade from Rust.
Long-term goal: remove Python CAD runtimes and external CAD app dependencies from
the product path after native OCCT proves coverage.

## Target

Rust owns the CAD runtime contract. OpenCascade stays behind a narrow native
boundary.

Pipeline:

```text
Ecky source
  -> Core IR
  -> Core normalization
  -> Direct OCCT plan
  -> native OCCT execution
  -> STEP + STL + topology.json
  -> Ecky runtime bundle + manifest
```

Rust responsibilities:

- parse and typecheck Ecky source.
- normalize dynamic Core IR into finite geometry graphs.
- build `OcctPlan`.
- validate operation support before native execution.
- generate stable IDs, manifests, cache keys, and raw error text.
- own parameter substitution and deterministic expansion.

Native OCCT responsibilities:

- create BREP shapes.
- apply booleans, transforms, fillets, chamfers, shells.
- write STEP/STL.
- enumerate topology into JSON.

Native boundary must stay small. Do not expose broad OCCT API into app code.

## Delivery Strategy

Do not start by deleting working paths.

Order:

1. Build native OCCT path beside existing render paths.
2. Prove real models render correctly through native OCCT.
3. Switch default exact render path only after tests and artifacts prove it.
4. Keep old paths available during burn-in.
5. Remove heavy dependencies only after native path owns required product surface.

Deletion is program maximum, not first milestone.

## Program Maximum: Dependency Purge

End state after proof gates:

- no bundled Python CAD runtime.
- no build-time `build123d` runtime preparation.
- no OCP Python package probing.
- no external FreeCAD command requirement.
- no Python CAD runner scripts in product resources.
- no user-visible backend choice for external CAD systems.
- one Rust-owned exact CAD runtime backed by OCCT libraries.

Possible code removals after direct OCCT reaches coverage:

- remove `runtime/build123d` resource entry from `src-tauri/tauri.conf.json`.
- remove `server/build123d_runner.py` and `_ecky_build123d_helpers.py` resources.
- remove `npm run build123d:prepare` from package scripts.
- replace `inspect_build123d_ocp_runtime` with direct OCCT SDK discovery.
- replace OCP include/dylib discovery with our own OCCT SDK layout.
- remove Python lowering/render dispatch from product path.
- remove FreeCAD command probing and UI config from product path.

Source migration tools can remain developer-only if needed. They must not ship in
the final app bundle.

## Proof Gates Before Removal

No dependency removal until all gates pass:

- native OCCT renders current direct OCCT fixtures.
- native OCCT renders one real generated model with parameters.
- native OCCT writes STEP, STL, `topology.json`, bundle, manifest.
- topology target IDs survive one edit cycle.
- SVG profile slice extrudes into a valid STEP.
- mesh/lithophane Rust path still works.
- raw native errors surface through UI.
- `cd src-tauri && cargo check` passes.

## Weight Budget

Observed local sizes:

```text
.dist/build123d-runtime                         1.6G
src-tauri/target/release/runtime/build123d      1.6G
src-tauri/target/release/runtime/speech          86M
src-tauri/target/release/ecky_cad                41M
src-tauri/target/release/bundle                 407M
```

Largest build123d runtime chunks:

```text
lib/                                            1.5G
site-packages/tree_sitter_language_pack         351M
site-packages/vtkmodules                        329M
site-packages/OCP                               254M
site-packages/OCP/.dylibs                        81M
site-packages/scipy                              97M
site-packages/sympy                              72M
include                                          47M
bin                                              31M
```

Target native OCCT package should be closer to required OCCT dylibs + headers for
build, not full Python/scipy/vtk/tree-sitter ecosystem.

Practical budget:

- app binary: current scale, around tens of MB.
- OCCT dylibs: expect tens to low hundreds of MB depending modules and stripping.
- no Python standard library, no site-packages, no VTK, no scipy, no sympy.

Primary saving: remove the 1.6G bundled Python CAD runtime.

## Existing Base

Current useful modules:

- `src-tauri/src/ecky_core_ir/mod.rs` - typed Core IR.
- `src-tauri/src/ecky_cad_host/direct_occt.rs` - Core IR to `OcctPlan`.
- `src-tauri/src/ecky_cad_host/direct_occt_executor.rs` - generated C++ OCCT
  executor.
- `src-tauri/src/ecky_cad_host/direct_occt_runtime.rs` - bundle/manifest wrapper.
- `src-tauri/src/runtime_capabilities.rs` - native SDK probing.

Current native executor already emits:

- primitives: `box`, `sphere`, `cylinder`, `cone`, `circle`, `rectangle`,
  `rounded-rect`, `rounded-polygon`, `polygon`, `profile`, `make-face`.
- solids/surfaces: `extrude`, `revolve`, `loft`, `sweep`, `twist`, `taper`,
  `offset`, `fillet`, `chamfer`, `shell`.
- paths: `path`, `bezier-path`, `bspline`.
- placement: `plane`, `location`, `path-frame`, `place`, `clip-box`.
- arrays: `linear-array`, `radial-array`, `grid-array`, `arc-array`.
- booleans/transforms: `union`, `difference`, `intersection`, `translate`,
  `rotate`, `scale`, `mirror`, `compound`.

This surface is enough for first-class exact CAD generation.

## Core Normalization

Direct OCCT should accept normalized Core IR only.

Normalization pass:

```text
CoreProgram + DesignParams -> NormalizedCoreProgram
```

Rules:

- `if` resolves when condition is scalar-evaluable.
- `range` expands to finite literal list.
- `map` expands over finite source lists.
- `apply` expands to explicit calls.
- `repeat` expands to explicit transformed instances.
- `repeat-union` expands to `union`.
- `repeat-compound` expands to `compound`.
- `repeat-pick` resolves selected generated item.
- scalar `let` bindings inline into runtime parameter environment when possible.
- unfilled `hole` rejects before planning.
- unsupported custom ops reject before native compile.

Failure text must name exact unsupported node and cause.

Example:

```text
Direct OCCT unsupported: `wall-pattern` is mesh-only and cannot lower to BREP.
```

## Plan ABI

Keep Rust-to-native ABI operation-centric.

Rust structs:

```text
OcctPlan
  parameters: Vec<OcctParameter>
  parts: Vec<OcctPartPlan>

OcctPartPlan
  key
  label
  root: OcctSlot
  commands: Vec<OcctCommand>

OcctCommand
  output: OcctSlot
  op: OcctOp
  args: Vec<OcctArg>
  keywords: Vec<OcctKeyword>
```

Native side should not parse Ecky source. Native side consumes already-planned
geometry commands only.

## Native Execution Strategy

Phase 1: generated C++ source per render.

- simple.
- debuggable.
- already implemented.
- compile cost acceptable for proving surface.

Phase 2: stable native runner.

- one compiled binary or dylib.
- JSON/binary command input.
- no per-render C++ compile.
- same `OcctPlan` contract.

Do not move app logic into C++. The runner should be replaceable.

## OCCT SDK Layout

Target resource layout:

```text
runtime/occt/
  include/opencascade/*.hxx       dev/build only if needed
  lib/libTK*.dylib                runtime
  licenses/
```

Runtime runner needs shared libraries only. Headers are needed to compile the
shim, not to execute it. Once Phase 2 runner is precompiled, headers can leave
the app bundle.

Minimum OCCT modules should be derived from includes in the executor:

- modeling data/kernel: `TKernel`, `TKMath`, `TKG2d`, `TKG3d`, `TKGeomBase`,
  `TKBRep`, `TKGeomAlgo`, `TKTopAlgo`, `TKPrim`, `TKBO`.
- modeling algorithms: `TKOffset`, `TKFillet`.
- exchange: `TKSTEP`, `TKSTEPBase`, `TKSTEPAttr`, `TKXSBase`.
- mesh/STL: `TKMesh`, `TKSTL`.

Exact dylib list must come from `otool -L` on the precompiled runner, then copy
only reachable deps.

## Topology

OCCT topology enumeration writes `topology.json` with:

- part id.
- face index, center, normal, area.
- edge index, start/end points.

Rust manifest layer derives public target IDs and aliases.

Rules:

- Native returns observed topology facts.
- Rust creates durable/public IDs.
- Selector matching for exact target IDs happens before native op emission.
- Broad selectors (`:normal`, `:area`, boundary clauses) should become Rust
  planning filters over topology snapshots or native query helpers, not ad hoc
  string matching.

## SVG Shape Path

Target SVG support: vector artwork becomes 2D BREP profiles, then normal Core IR
extrudes or cuts those profiles.

Not raster pixels. Vector SVG has paths and shapes. Pixels only matter as SVG
user units / viewBox scale. Raster images embedded in SVG are a different
heightfield/lithophane problem.

Target pipeline:

```text
SVG file
  -> resolved vector paths
  -> closed loops
  -> outer loops + hole loops
  -> Core `profile`
  -> Core `extrude` height
  -> OCCT face/solid
```

Example surface:

```text
(extrude
  (svg-profile "logo.svg" :width 24 :height 12 :fit contain)
  1.2)
```

Meaning:

- parse SVG vector geometry.
- flatten transforms.
- convert basic shapes to paths.
- fit geometry into 24 x 12 model units.
- create planar face loops.
- extrude 1.2 along Z.

## SVG Parser Choice

Use `usvg` for ingestion, not handwritten XML parsing.

Reason:

- resolves CSS/default/inherited attributes.
- converts basic shapes to paths.
- converts relative/implicit commands to absolute path segments.
- converts arcs to supported curve segments.
- exposes simplified path tree.

Use `svgtypes` only for narrow path-data parsing if `usvg` is too heavy.

Use `lyon` only for triangulation/mesh workflows. For BREP, prefer path loops
over triangle tessellation.

Sources:

- https://doc.servo.org/usvg/index.html
- https://doc.servo.org/svgtypes/path/struct.PathParser.html
- https://docs.rs/lyon_tessellation/

## SVG Loop Rules

Accept first slice:

- visible filled paths.
- `rect`, `circle`, `ellipse`, `polygon`, `polyline`, `path` after `usvg`
  conversion.
- `M`, `L`, `Q`, `C`, `Z` path segments from simplified path data.
- closed loops only for face creation.
- non-closed stroke-only paths only when caller asks for stroke width conversion.

Reject first slice:

- raster image nodes.
- filters/masks/clips that do not resolve to simple paths.
- text nodes unless converted to outlines by parser/preprocessor.
- open paths without stroke conversion.
- self-intersecting loops.
- ambiguous fill nesting.

Loop classification:

- group loops by connected SVG path/fill.
- compute signed area.
- largest positive/outer loop becomes face outer wire.
- contained opposite-orientation loops become holes.
- normalize orientation before native emission.

Curve strategy:

- line segment -> `BRepBuilderAPI_MakeEdge(gp_Pnt, gp_Pnt)`.
- quadratic Bezier -> convert to cubic or emit `Geom_BezierCurve`.
- cubic Bezier -> `Geom_BezierCurve`.
- arcs from SVG -> accept `usvg` converted cubic curves, or approximate to cubic
  segments.

## Units

SVG unit mapping:

- `viewBox` defines source coordinate system.
- `width`/`height` target args define model-space size.
- default model unit is millimeter-equivalent app unit.
- `fit contain` keeps aspect ratio inside target box.
- `fit cover` keeps aspect ratio and may exceed target box.
- `fit stretch` scales X/Y independently.
- Y axis flips by default because SVG Y grows downward, CAD Y grows upward.

Extrusion:

```text
height arg -> Z thickness
```

Do not map SVG pixel brightness to Z unless implementing explicit raster
heightfield/lithophane op.

## Native SVG Op Plan

Add Core primitive:

```text
svg-profile(path, width?, height?, fit?, flip_y?)
```

Lowering:

```text
svg-profile -> Profile { outer: Wire, holes: Vec<Wire> }
extrude(svg-profile, height) -> BRepPrimAPI_MakePrism(face, gp_Vec(0,0,height))
```

Implementation steps:

1. Add Rust `svg_profile` module.
2. Parse SVG with `usvg`.
3. Extract simplified visible paths.
4. Build Core profile loops or `OcctOp::Profile` inputs.
5. Add tests for rect, circle, path with hole, transformed group, viewBox fit.
6. Add native emitter support for Bezier loop wires if not enough today.
7. Add validation for open/self-intersecting paths.

## Exact CAD MVP

First production surface:

- normalized finite Core IR.
- primitive/profile/extrude/boolean/transform/array/frame ops.
- SVG profile ingestion.
- STEP/STL/topology bundle.
- raw errors.

Out of first production surface:

- raster heightmaps.
- full SVG rendering semantics.
- constraints.
- arbitrary file imports.
- live OCCT API exposure to frontend.

## Verification

For each added direct OCCT op:

- planner unit test.
- emitted C++ source test.
- native live test gated on SDK availability.
- runtime bundle manifest test.
- one fixture exercising parameter override.

For SVG:

- `svg_profile_rect_imports_as_face`.
- `svg_profile_viewbox_scales_to_target_width_height`.
- `svg_profile_hole_loop_becomes_profile_opening`.
- `svg_profile_rejects_raster_image`.
- `svg_profile_extrudes_to_step_when_sdk_ready`.

Run before success claim:

```text
cd src-tauri && cargo check
```
