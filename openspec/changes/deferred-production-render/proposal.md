## Why

Applying parameter changes or saving edits in the workbench currently triggers an expensive production render: full Direct OCCT evaluation, high-resolution STL meshing, STEP BRep generation, and topology metadata extraction. This blocks the authoring loop on heavy CAD math when the user only needs responsive 3D viewport feedback. Exact production artifacts (precise STEP, high-density print STL, FEA mesh) are only needed when exporting or running analysis.

## What Changes

- Separate rendering into two tiers: **Display Mesh** (fast, coarse tessellation using `kPreviewLinearDeflection` for interactive viewport editing and version persistence) and **Production Bundle** (exact STEP BRep, fine STL, export metadata materialized on demand).
- Update the **Apply** action for parameter and manual edits to materialize only the Display Mesh and commit the version immediately.
- Defer expensive STEP and fine STL generation to the **Export** action or on-demand download request.
- Add an architectural **Spike** to prove the boundary between the viewport display mesh and on-demand production artifacts: verifying visual fidelity, lifecycle state, cache isolation, and non-divergence between what the author sees and what exports.

## Capabilities

- `deferred-production-render` defines the lifecycle for display-first version creation and on-demand production artifact generation.

## Impact

- `src-tauri/src/services/render.rs`: Add display-first render mode; preserve exact CAD planner for export.
- `src-tauri/src/services/parameter_ghost.rs`: Generalize coarse tessellation pipeline into the standard display-tier render.
- `src/lib/types/domain.ts` and `src-tauri/src/contracts/render.rs`: Represent optional/pending production artifacts in `ArtifactBundle` without breaking existing consumers.
- Workbench: Parameter Apply becomes instant (~50–150ms instead of seconds).
