## Context

Ecky already has `SketchDocument`, projected-loop and hidden-line contracts, profile validation, FreeCAD front/top/side hidden-line extraction, and capture reconstruction. The main workbench currently disables the old sketch workspace. That component is a detached editor/preview workflow, not an authoritative source/version owner. Its helper can fall back from missing loops to a rectangle made from projection bounds, which is unsuitable as an authored profile. Capture reconstruction converts Object Capture meters to millimeters, while capture preview uses a separate scale factor; this split needs an explicit unit contract to avoid applying scale twice.

The existing project contract is authoritative: `.ecky` source is compiled and rendered, changed drafts become immutable versions, and Rust owns validation/persistence/orchestration. A sketch is an editing aid and evidence projection, not a second canonical model format.

## Goals / Non-Goals

**Goals:**

- Deliver a small end-to-end profile-to-solid authoring path that fits the current `.ecky` language and version services.
- Preserve analytic curves from supported exact BRep/STEP projections and sections, and bind approximate mesh sections to the source artifact, plane, and tolerance used to derive them.
- Preserve topology, provenance, and units across derivation, editing, and source generation.
- Fail safely when evidence cannot support a profile.
- Keep implementation inside existing sketch, geometry, capture, and version seams.

**Non-Goals for first release:**

- A general 2D CAD constraint solver or solver-backed parametric sketch model.
- Persistent independent sketch documents or a new sketch history/version lifecycle.
- Adaptive attachment to arbitrary faces, arbitrary projection planes, or a universal projection engine.
- Every feature operation or production drawing export.
- Claims that photogrammetry is dimensionally reliable without calibration and physical measurement.

## Decisions

### 1. `.ecky` remains the only authored model source

An edited sketch is translated into supported `.ecky` profile/extrusion source and submitted through the normal Rust source/version service. Existing user edits stay in their source/version history. Sketch geometry is transient until applied as a normal `.ecky` feature. Reopening a version restores its source, not an editable 2D sketch; arbitrary `.ecky`-to-sketch rehydration is deferred. Do not persist a second authoritative sketch file or invent a promote/finalize operation.

The first release adds a narrow `import-profile` source form backed by the native Direct OCCT runner. Exact section/projection derivation accepts eligible STEP/BRep artifacts and runs through bundled native OCCT, with no FreeCAD dependency or fallback. FCStd-only input is unsupported until a STEP artifact exists. Derivation writes an immutable content-addressed STEP face asset under app data (`profile-assets/<sha256>.step`). `.ecky` source stores that local path, so the asset survives version reopen for as long as the app-data file remains, but the source is not portable without its asset. Do not garbage-collect an asset while a saved source references it. Clearance and extrusion then operate on the exact profile through normal `.ecky` source and immutable version services. Manual and mesh-derived polygon profiles use existing `profile`, `offset`, and `extrude` forms. The new source form has explicit compiler/backend validation; it is not a frontend-only geometry shortcut.

### 2. One evidence envelope, explicit source quality

Exact projection/section results carry the source artifact path/digest, supported view/plane, tolerance, millimeter units, `exactBrep` accuracy, and a digest-addressed STEP profile-face asset that preserves BRep curves and holes. Sampled display loops are marked approximate-display-only; they are never sent as exact feature geometry. Approximate mesh section results carry `approximateMesh` accuracy and millimeter-valued point loops. The editor is scoped to the active thread/version/source; stale responses are dropped when that scope changes. Before saving, Rust checks the base message, expected source, source artifact digest, and exact profile-asset digest. The generated `.ecky` source imports the exact profile asset and records compact provenance for the accepted base version and projection/section settings.

Eligible STEP/BRep artifacts use native Direct OCCT for exact projection on supported canonical views and exact sectioning on supported canonical planes. FCStd-only sources need an existing STEP artifact; exact extraction must report an actionable unsupported-source error instead of invoking FreeCAD or falling back to STL. Mesh sectioning is approximate even when the mesh originated from photogrammetry or was exported from an exact model. A faceted STEP derived from a scan may have exact projections relative to its stored facets, while the physical object and its measured dimensions remain approximate; preserve its mesh provenance and show that limitation. Existing capture calibration provenance remains separate from geometric source class; calibration does not make a triangle mesh an exact BRep. Before applying an exact profile, validate that the target model's selected geometry backend can consume `import-profile`; report unsupported backends before creating a version.

### 3. Profile extraction accepts topology evidence, not just extents

Exact profile geometry remains in a digest-addressed STEP face/wire asset, so arbitrary supported BRep curves and holes survive without reducing the source to a line/arc DTO. Sampled loops are a display overlay only. Exact offset/clearance and extrusion import the authoritative profile asset, not display samples. The user may explicitly convert the displayed loops to an approximate polygon for point editing; this removes the exact asset reference and records the accuracy change. Approximate mesh and manual profiles use editable closed polygon loops. Open, self-intersecting, zero-area, non-finite, or ambiguous results return a specific diagnostic. Bounds may be displayed as measurements, but never become replacement profile geometry.

The current bounds-rectangle fallback remains unavailable to authoring. Existing consumers may use it only if their distinct preview contract explicitly permits a non-authoritative visual placeholder; no persisted source or editable profile can be produced from that placeholder.

### 4. First release supports exact CAD and approximate scan paths

Support exact profile derivation from eligible STEP/BRep projections and sections, and approximate profiles from mesh sections or manual closed polylines. Offer direct polygon editing for manual/approximate profiles and numeric clearance plus extrusion against the exact STEP profile asset. A derived view/plane and section offset fix the feature plane/location and stay read-only; changing the section location requires deriving again. Manual profiles may choose a principal plane and offset. Exact CAD operations do not consume sampled display loops. Validate clearance sign, finite values, self-intersection, collapsed loops, hole behavior, and extrusion distance before saving. Unsupported geometry or operation combinations report a clear limitation and leave prior source usable.

First release does not promise constraint-driven edit propagation. Constraints may be displayed as measurements or fixed relationships only if their meaning is exact and round-trips into source; otherwise they are deferred. The editor must not imply a solver exists.

### 5. Metric scale is converted once and uses existing calibration

Choose millimeters as the scan/profile unit at the reconstruction boundary. Object Capture meters convert to millimeters once; later preview and source-generation stages must treat reported dimensions as mm and must not reapply a legacy display/preview factor. Reuse existing capture calibration inputs and provenance; this change adds no separate calibration editor or reference-measurement flow. Do not rewrite stored scales for existing capture runs during migration; new/edited runs use the corrected path and old values remain readable with their original interpretation.

Uncalibrated scans remain labeled approximate. When an existing capture reference is invalid, retain the existing calibration diagnostic rather than silently inventing a reference. Critical dimensions still require independent measurement.

### 6. Rust owns status and lifecycle; frontend keeps stable layout

Rust services own source-version binding, projection/section work, geometry checks, persistence, and stale-state decisions. The frontend submits user intent and presents progress/result/error. It does not implement retries, version state machines, or source lifecycle facts. While projection, reconstruction, or validation is pending, reserve status space so the sketch canvas and controls do not shift. Failure states keep the existing model/source visible and explain the actionable cause.

The Sketch window retains the established front/top/side pane arrangement and per-pane camera controls. A single exact-projection action derives all three views from the currently selected model source. Each pane owns its projection result and error display; selecting a view binds its own profile reference and canonical plane to the apply request. The existing detached preview, constraint inspector, and candidate lifecycle are not restored as part of this integration; Rust's normal immutable version service remains the only save path.

### 7. Roadmap remains separate from first-release acceptance

Later work may add arbitrary-plane projection, a full constraint solver, adaptive face attachment, full feature operations, drawing exports, and improved scan reconstruction/diagnostics. Each addition must define its own source representation, round-trip semantics, immutable provenance, and acceptance scenarios. No first-release spec below implies these capabilities already exist or must ship as a single broad implementation.

## Risks / Trade-offs

- A supported first release depends on the current `.ecky` grammar representing derived profiles and extrusions; source inspection must confirm exact forms before implementation.
- Strict topology rejection can block some noisy mesh sections. This is safer than inventing a closed boundary; later reconstruction can offer repair as an explicit, reviewable operation.
- Exact CAD projection and sectioning depend on eligible STEP/BRep artifacts and native Direct OCCT; FCStd-only and unsupported backend combinations must report limitations instead of substituting a mesh result.
- Existing capture rows store preview scale. Compatibility requires preserving their values and keeping conversion distinct from display scaling until each legacy interpretation is understood.
- A calibrated scan still contains approximate mesh geometry. UI wording and provenance must preserve that distinction.

## Validation Approach

Define acceptance scenarios before implementation: exact STEP profile asset with curve types broader than a polygon DTO, projections and sections with holes; exact offset/clearance/extrusion using the asset and not display samples; explicit approximate polygon conversion; approximate mesh section labeling; open/branching topology rejection; stale source and asset detection; unsupported backend response; invalid offset/clearance or extrusion rejection; pending-state layout stability; scan meters-to-millimeters conversion; use of existing calibration provenance; and unchanged legacy capture scale on reopen. Follow project verification rules and do not claim geometry authoring completion without the inspect/validate/preview/verify sequence.
