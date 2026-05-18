# Proposal: Native OCCT Runtime

## Intent

Make Rust-owned direct OpenCascade rendering a proven product path before any
dependency removal.

Current direct OCCT work already has a planner, generated C++ executor, STEP/STL
output, and topology JSON. The weak spot is runtime ownership: SDK discovery is
still tied to the build123d/OCP Python runtime, dynamic Core IR reaches the
planner too late, SVG is still parsed through external CAD/Python paths, and the
native runner ABI is not separated from generated source compilation.

## Scope

- Add standalone OCCT SDK/runtime discovery beside current OCP discovery.
- Define native runner ABI for executing `OcctPlan`.
- Normalize Core IR before direct OCCT planning.
- Add Rust-native SVG profile ingestion for vector profiles.
- Strengthen topology/edit-cycle proof gates.
- Keep current working render paths intact during implementation.

## Out of Scope

- Removing bundled Python CAD runtimes.
- Removing external CAD command support.
- Replacing every existing CAD backend path in one change.
- Implementing raster heightfield/lithophane through SVG.
- Full SVG rendering semantics.
- Full OpenCascade API exposure to Rust application code.

## Approach

Use one umbrella OpenSpec change with independent workstreams:

- T1: standalone OCCT SDK packaging/probe.
- T2: precompiled native runner ABI.
- T3: Core IR normalization.
- T4: SVG profile ingestion.
- T5: topology/selector proof gates.
- T6: integration review and acceptance.

Each implementation worker gets a disjoint write scope. The main thread reviews
and integrates.

## Proof Before Removal

No dependency removal may happen until:

- native OCCT SDK path exports STEP/STL/topology without OCP Python package.
- direct OCCT renders current direct OCCT fixtures.
- one generated model with parameters renders through native path.
- SVG profile extrudes into valid STEP.
- topology IDs survive one edit cycle.
- mesh/lithophane Rust paths still work.
- raw native errors reach UI.
- `cd src-tauri && cargo check` passes.
