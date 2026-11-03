## Why

Ecky has projection, sketch validation, and photogrammetry reconstruction pieces, but no supported compact authoring path that joins them to ordinary `.ecky` source and immutable model versions. The legacy sketch workspace is detached from the main workbench, and its projection-to-sketch helper can invent a bounds rectangle when it cannot recover a closed loop. That can turn incomplete evidence into geometry that looks authored.

A staged profile-authoring workflow should let a user derive and edit a planar profile from an existing model or a calibrated scan, apply an offset or clearance, extrude it, and save the result through the normal source/version path. Evidence quality and provenance must remain visible so approximate mesh sections are not mistaken for exact BRep projections.

## What Changes

- Define a compact, source-backed sketch workflow that produces normal `.ecky` edits and immutable versions; sketches do not become a parallel project/history format.
- Support manual closed-polyline drawing and vertex editing for planar profiles.
- Make exact BRep/STEP projection and section evidence, and approximate mesh-section evidence, explicit about source version/artifact digest, view or plane, tolerance, units, and quality.
- Allow exact CAD projections/sections and closed, unambiguous mesh sections to seed profiles; preserve supported analytic curves, holes, and loop roles, and reject open, ambiguous, or stale evidence instead of fabricating bounds geometry.
- Add a focused profile operation path for offset/clearance and extrusion, with validation errors returned before saving invalid source.
- Correct photogrammetry scale handling so meter-to-millimeter conversion occurs once, preserve existing saved scales, use the existing capture calibration workflow, and warn that uncalibrated reconstruction is approximate.
- Keep a full constraint solver, arbitrary-face attachment/planes, broader feature operations, drawing exports, and reconstruction-quality improvements as later stages.

## Capabilities

### New Capabilities

- `sketch-authoring`: editable planar profile workflow connected to `.ecky` source, preview, and immutable version services.
- `projection-evidence`: provenance-bound exact CAD and approximate mesh projection/section evidence with topology-preserving profile derivation.
- `photogrammetry-scale`: metric scale conversion and compatibility with the existing capture calibration provenance.

### Modified Capabilities

- No existing registered OpenSpec capabilities. The change extends sketch contracts, exact CAD and mesh projection/section paths, capture scale handling, and normal model-version workflow without taking lifecycle ownership from Rust.

## Impact

Affected owners: the main workbench and compact sketch UI; exact CAD and mesh-section contracts; native OCCT projection/section and mesh-section services; `.ecky` source generation and validation; capture reconstruction and existing scan-scale provenance. Rust remains authoritative for source persistence, immutable version creation, rendering, and validation orchestration. The frontend submits edits and projects backend state; it does not create a second version lifecycle.

## Delivery Stages

**First release:** compact sketch editing tied to a normal `.ecky` version; exact supported BRep/STEP projections and sections with curve-aware profile evidence, plus approximate mesh sections for scans; offset/clearance and extrusion; clear exact-versus-approximate provenance; photogrammetry metric-scale correction using existing calibration inputs; safe rejection of open, ambiguous, stale, or unsupported evidence. Sketch edits remain transient until converted to a normal `.ecky` feature; reopening a version does not rehydrate it into the 2D editor.

**Later releases:** arbitrary-face projection/attachment, a full parametric constraint solver, full feature-operation coverage and drawing exports, and higher-quality photogrammetry reconstruction with better diagnostics. These are roadmap requirements, not claims about current behavior or first-release delivery.
