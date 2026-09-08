# Design: Hybrid Render Performance and Job Control

## Baseline

Dense hybrid renders may convert every mesh triangle into a planar OCCT face,
repeat identical kernel work, and clone large immutable artifact graphs through
preview layers. Long OCCT jobs expose no cooperative cancellation boundary.

Reference implementations motivate these mechanisms:

- OCCT Boolean builders support argument/tool groups, parallel execution, OBB
  filtering, progress, cancellation, and result simplification.
- CadQuery, build123d, and FreeCAD batch operands and clean results with
  same-domain unification.
- Manifold operates on validated indexed manifold meshes and warns that STL
  round-trips lose topology.
- meshoptimizer supports error-bounded simplification with achieved-error
  reporting and protected borders.
- OpenSCAD uses selective node caching instead of one undifferentiated scene
  cache.

Primary references:

- <https://dev.opencascade.org/doc/overview/html/specification__boolean_operations.html>
- <https://github.com/CadQuery/cadquery/blob/master/cadquery/occ_impl/shapes.py>
- <https://github.com/gumyr/build123d/blob/dev/src/build123d/topology/shape_core.py>
- <https://github.com/FreeCAD/FreeCAD/blob/main/src/Mod/Part/App/TopoShape.cpp>
- <https://github.com/elalish/manifold>
- <https://manifoldcad.org/docs/html/classmanifold_1_1_manifold.html>
- <https://github.com/zeux/meshoptimizer>
- <https://doc.cgal.org/latest/Surface_mesh_simplification/>

## Preserve Representation Until Required

Canonical hybrid mesh data is indexed, oriented, validated, and
content-addressed. STL remains an export format, not the internal cache or
handoff format.

Indexed sidecar schema v2 stores IEEE-754 vertex bits, indexed triangles, and
the content digest. Evaluated CAD meshes use a named 1e-6 mm seam weld before
admission. Explicit indexed assets retain supplied coordinates. The native
runner receives vertices and triangles directly.

- Exact BRep chains and analytic STEP stay in OCCT.
- Mesh islands targeting STL/3MF use a mesh Boolean kernel after local exact
  operands are tessellated.
- Faceted STEP uses the poly-BRep bridge only under an explicit face budget.
- Pure placement of imported meshes skips Boolean conversion.

Kernel admission happens before execution. Failure after Manifold starts is
reported raw; no hidden OCCT fallback runs.

Direct OCCT runner admission uses one plan validator for operation, argument,
keyword, and selector-form support. A rejected command identifies its part,
output slot, operation, and violated ABI constraint before the runner process
starts. Removal of generated-C++ fallback MUST NOT collapse distinct admission
failures into a generic unsupported-plan error.

## Boolean Planning

Union and head-minus-tail difference use one n-ary Boolean builder. OCCT
`Common` operates between two groups, so `head ∩ union(tail)` is not a valid
replacement for n-way intersection. Intersection keeps its proven fold until a
dedicated `BOPAlgo_CellsBuilder` implementation is verified.

Both OCCT execution paths enable `SetRunParallel(true)` and `SetUseOBB(true)`.
Input order remains part of plan identity. `SetNonDestructive(true)` requires a
memory/time benchmark before becoming default. Inverted-solid checks may only
be removed after validity and orientation proof.

Global glue is forbidden for intersecting geometry. Fuzzy tolerance requires a
named tolerance policy.

## Cleanup and Simplification

Same-domain cleanup may run only when benchmarked face reduction exceeds its
cost. It must preserve manifoldness, component count, bounds, volume, and
configured deviation.

Mesh simplification is explicit and decoration-only. It uses absolute
millimetre error, protected fit-zone vertices and borders, and records requested
and achieved error. Physical and fit geometry is never silently simplified.

## Immutable Cache and Singleflight

Cache identity includes source mesh digest, repair policy, transform, ordered
operation and operand digests, tessellation settings, backend version, and
OCCT/Manifold runtime version.

Cacheable stages:

1. validated indexed mesh;
2. optional simplified mesh;
3. solidified or unified faceted BRep;
4. completed hybrid island;
5. final verified artifact bundle.

Only successful immutable artifacts enter the bounded cache. Identical
concurrent renders share one in-flight computation. Failures are not cached and
are delivered raw to every subscriber.

## Progress and Cancellation Actor

Each kernel job runs behind a process actor. Fixed stages are `import`,
`validate`, `solidify`, `boolean`, `cleanup`, `mesh`, `verify`, and `export`.
Skipped work is explicit. Executed stages report count, elapsed milliseconds,
and total elapsed time.

OCCT `Message_ProgressIndicator` supplies cooperative progress and user break
where supported. Cancellation terminates an uncooperative child. Shared work
continues until its final subscriber cancels. Partial output never enters cache.

Interactive stdout and stderr remain in the terminal surface. General app logs
receive typed state only.

## Preview Payload Boundary

One immutable render snapshot owns source identity, parameters, backend,
artifact bundle, and manifest. Actor and MCP boundaries pass stable references
or lightweight projections instead of cloning and serializing the full graph.

Dense anonymous triangle-derived OCCT topology is not published as semantic
selectors. Authored, tagged, and analytic targets remain eager; dense mesh
picking uses a lazy indexed query with explicit truncation metadata.

Truncated source windows cannot be submitted as full macro replacement without
explicit acknowledgement.

## Execution Order

1. Establish a stable, provenance-recorded dense imported-mesh benchmark.
2. Finish Boolean planner measurements and cleanup policy.
3. Finish immutable staged caches and byte-budgeted eviction.
4. Add typed progress and cancellation.
5. Complete standalone indexed import and multipart mesh-native export.
6. Reduce preview aggregate payload.
7. Add decoration simplification only if benchmarks still justify it.
