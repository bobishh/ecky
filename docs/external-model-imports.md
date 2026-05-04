# External Model Import And Component Reuse

## Status

This is product/engineering doctrine for importing external CAD models, adding ports, and reusing them in component packages and assemblies.

Current implementation:

- FreeCAD `.FCStd` import exists. It persists the source `.FCStd`, produces preview STL, STEP, model manifest, and part artifacts.
- `.ecky` source can import STL via `(import-stl "...")` and SVG profile/path data via `(svg "...")` where the chosen backend supports it.
- build123d is a supported source language/runtime path.
- Exact topology handles exist for FreeCAD edge/face targets and Direct OCCT edge/face targets. Accepted component ports can preserve validated `targetIds` against accepted bundle edge/face targets.
- Generic exact/runtime artifact bundles can now be wrapped into portable component package projects. Packaging prefers reusable source (`.ecky`, build123d `.py`, legacy Python) when the bundle exposes `macroPath`, falls back to STEP when needed, auto-derives full source param surface (`params`, `uiSpec`, `initialParams`) from reusable source when explicit package params are omitted, preserves explicit package-request `uiSpec` and `initialParams` for non-source-backed bundles, allows zero-port decorative/source-backed components, validates explicit port `targetIds` against runtime manifest/bundle topology before writing the package project, backfills empty param surfaces for installed legacy source-backed packages on resolve, merges stored `initialParams` with runtime overrides during installed component and assembly render, exposes cheap installed-component and installed-assembly controls resolve without rendering, and echoes merged values back from installed-component runtime responses.
- Component package contracts exist: params, ports, custom port types, mate types, assembly recipes, operations, keepouts, fusion zones, package header, payload archive, local install, and package browser header listing.
- Assembly/fuse/mold contracts exist, but the geometric solver, assembly editor, boolean execution path, and validation overlays are parked.

Not implemented yet:

- OpenSCAD `.scad` ingestion.
- STEP/native BRep package import as a first-class component creation flow.
- Automatic reliable port extraction from arbitrary imported geometry.
- Joined/fused assembly output from package instances.
- Source conversion from OpenSCAD/build123d/FreeCAD into canonical `.ecky`.

## Existing Plan Anchors

External import is not yet its own roadmap slice, but the needed pieces are already planned.

- `component-library-sketch-roadmap.md`
  - Defines component, port, mate, assembly, and operations.
  - Marks package contracts and local package install/import done.
  - Marks package detail/library UI and assembly/fuse/mold UI parked.
  - Phase 9 says save accepted model as component remains next.
- `ecky-language-improvement-roadmap.md`
  - Defines future source-level `component`, `port`, and `assembly` syntax.
  - Defines first port families: plane, axis, bolt pattern, dovetail, snap fit, socket, insert, surface patch, cut profile.
- `ecky-rust-cad-vm-roadmap.md`
  - Defines `.ecky -> Core IR -> backend -> artifact -> verification`.
  - Separates `.ecky` source from geometry backends: mesh/eckyRust, build123d, FreeCAD, future direct OCCT.
  - Defines typed holes as planning placeholders that must fail before render if unfilled.

This document fills the missing bridge: external model -> semantic component -> ports -> assembly reuse.

## Goal

Imported models should become reusable components without pretending that raw geometry contains enough meaning.

Correct flow:

```text
external source or artifact
  -> import bundle
  -> inspect/render/verify
  -> semantic wrapper
  -> component package
  -> assembly recipe
  -> optional joined/fused/molded output
```

Semantic wrapper means:

- stable params
- named ports
- mate compatibility
- keepouts
- optional fusion zones
- validation evidence
- source/artifact provenance
- license/dependency metadata

## Source Types

| Input | Current support | Best use | Port strategy | Rewrite strategy |
| --- | --- | --- | --- | --- |
| `.ecky` | Native source | Best long-term reusable source | Author ports directly in source/package manifest | No rewrite needed |
| FreeCAD `.FCStd` | Import exists | Exact BRep, part extraction, hidden-line validation | Suggest ports from BRep features and user/agent annotations | Wrap first; rewrite only if editable source is required |
| FreeCAD Python macro | Legacy source path exists | FreeCAD-owned exact CAD | Prefer explicit metadata comments or generated manifest | Preserve source; translate small stable subset later |
| build123d `.py` | Supported source language | Exact source-backed CAD with OCCT | Prefer source annotations and generated package manifest | Preserve source; translate to `.ecky` only for portability |
| STEP | Artifact support exists in BRep validation/export paths | Exact exchange geometry | Detect faces, axes, holes, and bolt patterns as suggestions | Wrap artifact; no param recovery expected |
| STL/mesh | `(import-stl "...")` exists | Preview, scan, bought mesh, mesh-only print asset | Manual ports only, or very weak suggestions from bounds/planes | Do not use as authoritative editable CAD |
| SVG | `(svg "...")` exists | 2D profiles/logos/sketch seeds | Usually not a component by itself | Convert profile to `.ecky` sketch when practical |
| OpenSCAD `.scad` | Not implemented | Existing script libraries and parametric modules | Explicit module metadata or manual port editor | Wrap source first; optional subset translation later |

## Import Pipeline

### 1. Ingest

Importer accepts source/artifact plus declared origin:

- `sourceKind`: `ecky`, `freecad`, `build123d`, `openscad`, `step`, `stl`, `svg`
- `sourceRef`: local package path, app artifact path, or original import path
- `contentHash`
- dependency list
- license
- runtime needed to rebuild

For `.FCStd`, current implementation already stores source artifact and derived artifacts.

For future OpenSCAD, import must store:

- root `.scad`
- vendored libraries/includes
- chosen parameter defaults
- OpenSCAD version
- generated artifact paths
- raw render stderr/stdout on failure

### 2. Render And Inspect

Importer renders or inspects enough to know what exists:

- preview STL or mesh
- STEP/FCStd when exact BRep exists
- `modelManifest`
- part list
- bounding boxes
- volume/surface metrics where available
- export artifact availability
- hidden-line or projection evidence for exact CAD

No assembly claim should be made from a mesh-only import unless it has manual semantic metadata.

### 3. Normalize Params

Params come from source when possible.

Priority:

1. `.ecky` params.
2. build123d/OpenSCAD explicit parameter declarations.
3. FreeCAD spreadsheet/named constraints/part metadata.
4. user/agent accepted bindings.
5. no params.

Param rule: same key must mean same thing across component versions. Do not reuse `width` once it meant rail width to later mean screw spacing.

### 4. Propose Ports

Importer may propose ports. It must not silently finalize them.

Reliable sources:

- explicit component metadata
- `.ecky` future `port` forms
- package manifest port definitions
- source comments with known schema
- named FreeCAD datum objects/sketches/spreadsheets
- build123d/OpenSCAD annotations

Heuristic sources:

- circular through-holes
- bolt-hole patterns
- coaxial cylinders
- planar mounting faces
- dovetail-like matching profiles
- repeated slot geometry
- BRep hidden-line evidence
- part names such as `mount`, `rail`, `slot`, `insert`

Heuristic ports must carry provenance and require accept/edit before component package save.

Stroke-first authoring follows the same rule. Thickened strokes, swept paths, and continuous-body drafts may suggest datums, profiles, keepouts, or ports from endpoints, junctions, centerlines, and generated faces, but those interfaces are not authoritative until accepted. Shape comes from strokes; assembly meaning comes from named ports, frames, compatibility rules, and validation evidence.

### 5. Wrap As Component

Wrapper records semantic contract around source/artifacts:

```json
{
  "componentId": "imported-bottle-cage",
  "version": "0.1.0",
  "displayName": "Imported Bottle Cage",
  "sourceRef": "artifacts/imported-fcstd-abc123/model.FCStd",
  "params": [],
  "ports": [
    {
      "portId": "seat_tube_bolts",
      "typeId": "mechanical.boltPattern.m5.twoBolt.v1",
      "frame": {
        "origin": [0.0, -32.0, 0.0],
        "xAxis": [1.0, 0.0, 0.0],
        "yAxis": [0.0, 1.0, 0.0],
        "zAxis": [0.0, 0.0, 1.0]
      },
      "params": {
        "diameter": 5.2,
        "spacing": 64.0,
        "clearance": 0.2
      },
      "interfaces": ["mechanical_mount", "bolt_pattern"],
      "compatibleWith": ["mechanical.boltPattern.m5.twoBoss.v1"],
      "allowedOps": ["mate", "cut"]
    }
  ]
}
```

Rust owns snake_case structs and serializes camelCase JSON through `#[serde(rename_all = "camelCase")]`. Frontend payloads stay camelCase.

### 6. Save Package

Package archive contains:

- `ecky-header.json`
- `ecky-payload.b64`
- inner `ecky-package.json`
- source files/artifacts
- previews/exports
- metadata/provenance

Header should expose only safe package interface data:

- package id/version/display name
- tags/visibility
- port types and mate types
- component names
- exposed params
- port summaries

Header must not require full payload read.

### 7. Reuse In Assembly

Assembly uses package component instances plus mates/operations:

```json
{
  "assemblyId": "bike-cage-on-frame",
  "displayName": "Bike Cage On Frame",
  "components": [
    { "instanceId": "frame", "componentId": "frame-rail" },
    { "instanceId": "cage", "componentId": "imported-bottle-cage" }
  ],
  "mates": [
    {
      "mateId": "cage_to_frame",
      "typeId": "mechanical.boltPattern.m5.v1",
      "a": { "instanceId": "frame", "portId": "bottle_bosses" },
      "b": { "instanceId": "cage", "portId": "seat_tube_bolts" },
      "params": { "clearance": 0.2 }
    }
  ],
  "operations": [
    {
      "operationId": "place_cage",
      "kind": "mate",
      "targetInstanceIds": ["frame", "cage"],
      "portRefs": [
        { "instanceId": "frame", "portId": "bottle_bosses" },
        { "instanceId": "cage", "portId": "seat_tube_bolts" }
      ]
    }
  ],
  "output": { "mode": "separateParts" }
}
```

Current contracts support this shape. Runtime assembly render now resolves installed component sources, renders separate-part instance runtimes, solves first-pass exact port-frame placement for mated instances via per-instance `placementFrame`, returns per-mate solve evidence with first clearance-rule checks, returns per-operation apply evidence with group ids, exact warnings, and `fusionZoneIdsByInstance`, blocks joined/fused runtime execution when targeted instances do not expose op-capable fusion zones, can synthesize a joined assembly runtime bundle when `output.mode = JoinedAssembly` and operations are absent, pure `Fuse` groups, or narrow ordered `Cut` groups (`targetInstanceIds[0]` base, `targetInstanceIds[1..]` cutters), can synthesize a narrow fused runtime bundle when `output.mode = FusedSolid` and pure `Fuse` groups cover the whole assembly, and can export solved separate-parts assemblies as placed 3MF build items or multipart STL zips with baked placement transforms. Richer boolean operations, richer mate rules, and molded assembly output remain parked.

## Ports

Port is component interface, not a marker point.

Minimum port data:

- `portId`
- `typeId`
- `targetIds` when the port is anchored to accepted geometry selections
- local frame
- params
- compatible port types
- allowed operations
- validation evidence

Recommended extended data:

- side: `male`, `female`, `neutral`
- profile reference
- clearance
- insertion axis
- stop plane
- keepout volume
- fusion surface
- tolerance
- source evidence

Port frame convention:

- `origin`: mating datum center.
- `zAxis`: outward normal or insertion direction.
- `xAxis`: alignment direction around z.
- `yAxis`: derived orthogonal axis.

Examples:

- bolt pattern: origin at pattern center, z normal to mounting face, x through first/second bolt.
- dovetail rail: origin at rail datum, z slide axis, x profile width axis.
- socket: origin at socket mouth center, z insertion axis.
- surface patch: origin at patch centroid, z outward face normal.

## Holes

"Hole" has two different meanings in Ecky. Keep them separate.

### Runtime Geometry Holes

These are real geometry:

- circular cutouts
- `profile` holes in sketches
- boolean `difference`
- imported BRep through-holes
- bolt clearance holes

Runtime geometry holes can become ports when they are stable interfaces.

Example:

- two M5 clearance holes spaced 64 mm become `mechanical.boltPattern.m5.twoBolt.v1`
- one cylindrical socket becomes `mechanical.socket.round.v1`

### Typed Holes

Typed holes are planning placeholders in CAD VM source.

They are not renderable geometry.

Rule:

- typed holes may compile/typecheck as missing design intent
- typed holes must fail before backend render if unfilled
- imported components must not save unfilled typed holes as final source

Imported geometry should turn holes into one of these:

- actual cut geometry in source
- detected BRep feature
- manual port
- keepout/cut volume
- validation warning

Never treat an unfilled typed `(hole ...)` as a reusable port.

## Assembly Modes

### Separate Parts

Use for packaging, BOM, jig setups, and preview assemblies.

Behavior:

- solve transforms from mates
- preserve original components
- export multipart compound where backend supports it
- no boolean operations

This is safest first assembly target.

### Joined Assembly

Use when components touch or are grouped but should remain separate solids.

Behavior:

- mate/place instances
- preserve identity
- allow joined export metadata
- no destructive source rewrite

### Fused Solid

Use when final output should be one printable/manufacturable solid.

Requires:

- exact BRep or source-backed solids
- fusion zones
- keepouts
- operation order
- post-boolean validation
- raw backend error with operation context on failure

Do not blindly fuse mesh-only STL imports.

### Molded Solid

Use when Ecky creates transition geometry between parts, unions bodies, then blends.

Requires:

- declared fusion surfaces
- maximum blend radius
- wall thickness limits
- material/print constraints
- forbidden keepout intersections
- validation overlays

Molded output should stay unavailable until solver and boolean validation are real.

## OpenSCAD Import Plan

OpenSCAD should start as source wrapping, not translation.

Phase 1:

- accept `.scad`
- capture include/use dependency closure
- store chosen parameter defaults
- run OpenSCAD to export STL and, when available, better exchange artifacts
- store raw stdout/stderr
- create component wrapper with no auto-final ports

Phase 2:

- read module/parameter names
- support optional metadata comments
- propose ports from metadata
- package source and dependencies

Example metadata comment:

```scad
// ecky:component id="m5_knob" version="0.1.0"
// ecky:param name="diameter" unit="mm" default=32
// ecky:port id="thread_insert" type="mechanical.insert.m5.v1" origin=[0,0,0] zAxis=[0,0,1]
module knob(diameter = 32, height = 12) {
  // OpenSCAD body
}
```

Phase 3:

- translate safe subset into `.ecky`
- support modules, params, transforms, booleans, simple primitives, loops with bounded literal ranges
- reject dynamic or unsupported constructs with exact errors
- preserve original `.scad` as sourceRef even after translation

OpenSCAD libraries should not become Ecky catalogs by automatic rewrite. Wrap useful modules first. Rewrite only modules where portability/editability matters.

## build123d Import Plan

build123d should be treated as source-backed exact CAD.

Phase 1:

- accept `.py`
- run in bundled/runtime-selected build123d environment
- capture primary solid(s), preview STL, STEP, manifest, raw errors
- preserve script as `sourceRef`
- expose discovered parameters only when explicit or safely declared

Phase 2:

- support metadata comments or small Python-side manifest export
- propose ports from declared metadata and BRep inspection
- save as component package

Recommended build123d metadata pattern:

```python
# ecky:component id="hinge_leaf" version="0.1.0"
# ecky:param name="width" unit="mm" default=28
# ecky:port id="pin_axis" type="mechanical.axis.hinge.v1" origin=[0,0,0] zAxis=[0,1,0]
```

Do not introspect arbitrary Python for hidden semantics. Execute/render for geometry, read metadata for intent.

## FreeCAD Import Plan

`.FCStd` import already exists as inspect/import flow.

Next steps:

- create "Save As Component" from imported FCStd version
- map imported parts to component parts
- expose accepted FreeCAD parameter bindings as component params
- suggest ports from named datums, sketches, spreadsheets, hole axes, and mounting faces
- save source artifact plus STEP/preview artifacts into package payload
- show component header in package browser

FreeCAD object naming should become first-class evidence:

- `Datum_mount_face`
- `Axis_pin`
- `Sketch_bolt_pattern`
- `Spreadsheet_Params`
- `Port_seat_tube_bolts`

If names follow a known convention, importer can propose ports with high confidence. User or agent still accepts them before package save.

## MCP And Agent Behavior

MCP agents can help with imported models, but Ecky remains source of truth.

Agent may:

- import file
- inspect artifact manifest
- render mutations
- compare artifacts
- propose params/ports
- explain raw backend errors
- generate component package manifest draft

Agent must not:

- claim port correctness without artifact/source evidence
- silently accept heuristic ports
- dump terminal output into app logs
- convert mesh to exact editable CAD without validation
- promise assembly/fuse/mold output while UI/runtime is parked

Accepted import enrichment must persist in Ecky version/package state, not only agent memory.

## Validation

Imported component save should require:

- readable source/artifact reference
- preview artifact
- exact artifact when exact operations are requested
- unique component id/version
- stable param keys
- unique port ids
- known port types
- port `targetIds` reference accepted BRep selection targets when a port is tied to a real edge, rim, face, or datum instead of only a free frame
- accepted BRep component packaging can also preserve explicit control surface (`params`, `uiSpec`, `initialParams`) instead of hardcoding an empty one
- compatible mate declarations
- allowed operation checks
- keepout/fusion zone references valid
- source/license metadata present when package is exported

Port validation should report:

- port id
- type id
- source evidence
- frame axes normalized and orthogonal
- frame origin inside expected bounds or on expected face
- referenced feature exists
- tolerance/clearance rule result
- compatible mate type result

Assembly validation should report:

- missing component/package
- missing port
- incompatible port types
- impossible mate solve
- invalid operation target
- boolean failure with operation id and raw backend error
- keepout collision
- fusion zone missing

## Rewrite Policy

Default: wrap first, rewrite later.

Wrap when:

- model imports and renders
- source/license must be preserved
- user needs library reuse now
- params/ports can be added as metadata

Rewrite when:

- source is small and stable
- portability across FreeCAD/build123d/mesh matters
- user needs rich editable params
- source language blocks validation
- original source is unavailable but exact BRep evidence is good enough for a limited reconstruction

Do not rewrite when:

- model is mesh-only and complex
- license forbids derivative source
- external library behavior is too dynamic
- feature recognition is speculative
- assembly only needs placement/preview

## Minimal Implementation Slices

1. Save imported FCStd as component package.
2. Manual port editor for imported component.
3. Port validation from current BRep/sketch evidence. Backend accepted candidate-cell package builder exists for explicit ports and known port types; UI editor remains.
4. Package detail UI with preview, params, and port list.
5. Assembly editor for `place` and first `mate` type.
6. Separate-parts assembly export.
7. STEP import wrapper.
8. build123d `.py` component wrapper.
9. OpenSCAD `.scad` wrapper with dependency capture.
10. Fused output with fusion zones and post-boolean validation.
11. Molded output with transition geometry and blend validation.
12. Optional source translation into `.ecky` for safe subsets.

## Decision

External libraries are useful as source/artifact catalogs. Ecky should not clone them blindly into its own language.

Correct reuse model:

```text
library part
  -> wrapped component
  -> explicit ports
  -> validated mates
  -> assembly recipe
  -> optional rewrite only where value is clear
```

This keeps imported geometry useful immediately while preserving the CAD VM rule: only source with explicit, validated semantics can drive assembly, fuse, mold, and safe reuse.
