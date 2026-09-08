## Context

`make_path_wire` emits one linear edge per authored segment. `make_bezier_path_wire`
emits 16 linear edges per cubic segment to avoid an OCCT RTTI boundary defect.
`sweep_shape` previously selected every second point from any all-linear wire. That
selection is not valid for authored polylines because the omitted point can be a
direction-changing corner.

## Decisions

### Preserve linear vertices

Collect edge endpoints in wire order and pass the complete deduplicated sequence to
section construction. A linear wire vertex is authoritative path geometry. No
stride-based decimation is allowed.

### Simplify straight samples and keep sections bounded

Remove an interior point only when its adjacent vectors are forward-collinear
within native numeric tolerance. Direction-changing vertices remain untouched.
Set a native maximum of 1024 remaining linear section points. If the simplified
sequence is larger, throw
`EvalError("sweep path has too many linear sections (maximum 1024)")` before
`BRepOffsetAPI_ThruSections` is created. The 1024 budget covers authored
multi-row, multi-loop noodle workloads at the existing 16 samples per curved
cubic after straight spans collapse. This preserves corners and keeps loft work
deterministic without rejecting the real spoon-rest path.

### Preserve corners with ruled lofts

Use ruled `BRepOffsetAPI_ThruSections` for the linear branch. Each consecutive pair
of transported profiles forms one straight span, so a sharp path corner remains in
the generated sweep. Smooth spline lofting is rejected for this branch because it
can overshoot sharply changing tangents.

### Approximation semantics

The existing cubic Bézier representation remains a 16-edge-per-segment polyline.
Those samples are retained as linear section points except for numerically
collinear interior samples, while under the 1024-point bound. Thus the result
approximates the Bézier centerline and has round profile sections at each retained
station; it is not an analytic Bézier sweep. A path whose non-collinear sampling
exceeds the bound fails explicitly. Frenet/helix handling remains on its existing
dedicated branch.

## Verification

The native integration executable measures solid validity, bounds, volume, circular
edge radii, and an interior plane intersection. It covers a three-point corner, a
five-point polyline, one sampled cubic with a radius-4.5 profile (9-unit circular
diameter), and a 1025-point non-collinear bounded-work failure.

Complete artifact cache identity advances from v10 to v11. This prevents unchanged
source from reusing a pre-fix sweep bundle before the runner-aware geometry cache
is consulted. The targeted cache test fails with v10 and passes with v11.
