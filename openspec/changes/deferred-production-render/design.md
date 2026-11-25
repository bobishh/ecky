## Decisions

### 1. Two-tier rendering contract

- **Display Tier (Interactive / Viewport)**:
  - Generates lightweight STL/OBJ using coarse deflection (`kPreviewLinearDeflection` = 0.5mm, angular 0.5 rad).
  - Skips STEP BRep generation, STEP file writing, and expensive topology metadata.
  - Runtime budget: <150ms for typical models.
  - Stored in the version bundle as `modelStlPath`.

- **Production Tier (Export / Analysis)**:
  - Materialized strictly on demand when user clicks Export (STEP, 3MF, high-res STL) or requests analysis.
  - Runs full precision tessellation (`kStlLinearDeflection` = 0.05mm) and writes exact STEP BRep via OpenCASCADE.
  - Production artifacts are cached alongside the version by source/parameter digest.

### 2. Version commit on Apply

When the user modifies parameters and clicks **Apply**:
1. The backend evaluates the changed part plans and renders the **Display Mesh**.
2. The version is committed to SQLite and history immediately with `modelStlPath` populated and `modelStepPath: null` (flagged `productionArtifactsPending: true`).
3. Viewport updates instantly from the new display STL.
4. If the user never exports this revision, no CPU cycles or disk space are spent generating STEP/high-density meshes.

### 3. On-demand materialization during Export

When the user requests STEP export or production STL download:
1. The export service checks if `modelStepPath` exists in the version bundle.
2. If missing, it invokes `materialize_production_bundle(version_id)`.
3. The exact BRep is generated from the saved source and parameter values, written to disk, and updated in the bundle.
4. The requested export artifact is returned to the user.

### 4. Spike: Seam between display mesh and production BRep

Before full rollout, execute a verification spike:
- Measure latency difference between Display Tier and Production Tier on the Worm Farm benchmarks.
- Verify visual fidelity: ensure display mesh silhouettes match production BRep within the 0.5mm tolerance without misleading edge artifacts.
- Verify non-divergence: ensure deferred STEP generation from saved parameters produces identical geometry to immediate generation.
