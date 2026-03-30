# Build123d Backend Refactor Plan

## Goal

Refactor the generated-model stack so `Ecky IR` becomes the primary authoring language, while
geometry execution becomes a lower-level backend choice.

Target shape:

- authoring language:
  - `legacyPython`
  - `eckyIrV0`
- geometry backend:
  - `freecad`
  - `build123d`
  - `eckyRust`

Practical intent:

- keep `FreeCAD` for imported `FCStd` and compatibility work
- keep `legacy Python` only as a transitional generated lane
- make `Ecky IR` the main generated authoring surface
- plug `build123d` in as the first serious alternate geometry backend for `Ecky IR`

## Why

Current `engineKind` mixes two different concerns:

- what the LLM writes
- what actually renders geometry

That creates a bad shape:

- `freecad` means both “legacy Python style” and “run through FreeCAD”
- `eckyIrV0` means both “IR syntax” and “run through Rust mesh backend”

The refactor should separate these layers.

## New Layering

### 1. Source Language

Add a first-class source language enum:

- `LegacyPython`
- `EckyIrV0`

This should be the canonical language selection for generated modeling.

`macroDialect` can either evolve into this role or be replaced by a cleaner enum if the current
type is too overloaded.

### 2. Geometry Backend

Add a lower-level geometry backend enum:

- `Freecad`
- `Build123d`
- `EckyRust`

This is execution-only. It should not leak into prompt wording as if it were the source language.

### 3. Backend Resolution

Generated render path should resolve as:

- source language
- backend override
- compatibility rules

Example:

- `LegacyPython` defaults to `Freecad`
- `EckyIrV0` defaults to `Build123d` or `EckyRust`, depending on config
- imported `FCStd` stays pinned to `Freecad`

## Refactor Steps

### Phase 1. Untangle contracts and persistence

Add distinct persisted fields for:

- `sourceLanguage`
- `geometryBackend`

Touch points:

- `src-tauri/src/contracts.rs`
- `src/lib/tauri/contracts.ts`
- `src/lib/types/domain.ts`
- thread metadata
- design output
- artifact bundle
- model manifest
- history/inventory serialization

Rules:

- existing `engineKind` data must migrate cleanly
- old `freecad` threads should map to:
  - `sourceLanguage = legacyPython`
  - `geometryBackend = freecad`
- old `eckyIrV0` threads should map to:
  - `sourceLanguage = eckyIrV0`
  - `geometryBackend = eckyRust`

### Phase 2. Move UI selection up to source language

Replace the current main thread-level “engine” choice with a source-language choice:

- `LEGACY PYTHON`
- `ECKY IR`

Backend choice should move to advanced config or dev-only override.

Touch points:

- `src/lib/PromptPanel.svelte`
- `src/lib/ConfigPanel.svelte`
- `src/App.svelte`
- thread history / version badges

Behavior:

- thread surface should show source language prominently
- geometry backend can be visible in advanced metadata, but not as the main creative choice

### Phase 3. Introduce build123d backend interface

Create a backend boundary instead of branching inline in render service.

Suggested shape:

- `render_generated_model(language, backend, source, params, post_processing, ...)`
- backend modules implement a shared result contract returning:
  - `ArtifactBundle`
  - `ModelManifest`
  - part bounds
  - multipart assets

Touch points:

- `src-tauri/src/services/render.rs`
- `src-tauri/src/freecad.rs`
- `src-tauri/src/ecky_ir.rs`

Do not keep the current “if IR use Rust else FreeCAD” branching shape once this refactor starts.

### Phase 4. Add build123d runner for generated code

Introduce a new module, likely something like:

- `src-tauri/src/build123d.rs`

Responsibilities:

- execute generated `build123d` code in a controlled process
- export preview STL
- export per-part STL assets
- emit manifest metadata compatible with current viewer/runtime
- surface raw backend errors verbatim

Important:

- this should not depend on FreeCAD app/runtime
- if `build123d` still uses OCCT through Python, that is acceptable
- the goal is to remove `FreeCAD` as the app shell, not necessarily Python on day one

### Phase 5. Lower Ecky IR into build123d

Add a new lowering path:

- `Ecky IR AST -> build123d Python code`

This should be explicit and testable.

Suggested structure:

- keep parser/validation in Rust
- lower validated IR into a compact generated Python program for `build123d`
- then execute that through the `build123d` backend module

Do not bypass IR validation by letting the LLM write raw `build123d` directly in the normal
`Ecky IR` lane.

### Phase 6. Keep direct Rust backend alive in parallel

Do not delete the current `ecky_ir.rs` execution path immediately.

Use it as:

- comparison backend
- home for mesh-first operations that fit Rust better than `build123d`

Expected split:

- `build123d` backend for profile/solid/OCCT-friendly geometry
- `eckyRust` backend for mesh/pattern/lithophane-heavy paths where direct mesh control is better

### Phase 7. Move post-processing above backend

Keep attachment-based lithophane and export logic above the geometry backend boundary.

Invariant:

- backend produces bundle + manifest
- post-processing consumes bundle + manifest

That keeps:

- lithophane attachments
- multipart STL zip
- 3MF packaging
- preview safety gates

backend-agnostic.

## Explicit Non-Goals

This refactor does not include:

- deleting the current Rust IR renderer
- replacing imported `FCStd` workflows
- generic CAD assemblies
- TechDraw/FEM/CAM/BIM lanes
- generic solid fillet/chamfer kernel work

## Migration Plan

### Step A. Introduce new enums and dual-write metadata

Before removing `engineKind`, write both:

- old field
- new `sourceLanguage`
- new `geometryBackend`

This allows a rolling migration without breaking history.

### Step B. Refactor render dispatch behind one backend boundary

Get to one place where generated rendering is chosen by:

- source language
- backend

Only after that should backend count grow.

### Step C. Land build123d backend in shadow mode

Add `build123d` backend without making it the default.

Use it only behind config / dev override at first.

### Step D. Enable `Ecky IR -> build123d`

Once lowering is stable for a meaningful subset, allow:

- new IR threads to choose `build123d` backend

Keep fallback explicit.

### Step E. Re-evaluate legacy Python

After `Ecky IR -> build123d` is usable on real models, decide whether raw legacy Python generation
still deserves first-class support or should become compatibility-only.

## Required Tests

### Contracts / persistence

- old thread metadata migrates to new source/backend fields
- history and inventory preserve both source language and backend truthfully

### Render dispatch

- `LegacyPython + Freecad` still works
- `EckyIrV0 + EckyRust` still works
- `EckyIrV0 + Build123d` routes to the new backend
- imported `FCStd` remains pinned to FreeCAD

### Lowering

- IR sample shapes lower deterministically into build123d code
- lowering rejects unsupported nodes explicitly

### Backend output

- build123d backend returns preview STL + viewer assets + manifest
- multipart models preserve part ids/order
- post-processing still works on returned bundles

### Regression set

Use at least:

- lamp shade
- bulb shell
- planter/hydroponic shell
- simple bracket
- lithophane attachment on an IR-generated part

## Questions To Resolve Before Coding

### 1. Is build123d only a backend, or also a direct authoring surface?

Recommended answer:

- backend only
- do not let normal generated flow drift into raw build123d Python

### 2. Does `LegacyPython` need a non-FreeCAD backend?

Recommended answer:

- no
- keep `LegacyPython -> Freecad`
- only `Ecky IR` gets multiple backends

### 3. What should become the default backend for IR?

Recommended answer:

- keep `eckyRust` as default until build123d parity is proven
- then decide based on real model runs

## First Concrete Slice

If this starts now, the smallest worthwhile slice is:

1. Add `sourceLanguage` + `geometryBackend` metadata
2. Refactor render dispatch to use them
3. Add a stub `build123d` backend module returning explicit “not implemented”
4. Wire `Ecky IR -> build123d` dispatch path
5. Add one real IR lowering case:
   - `rounded_rect`
   - `extrude`
   - `difference`
6. Produce preview STL + basic manifest from build123d backend

That gets the architecture in place before the bigger lowering work starts.
