# Proposal: SVG/Text Native Exact-Curve Parity

## Intent

Close the performance and fidelity gap between the Direct OCCT (Ecky Native)
backend and the build123d backend for `(svg ...)` and `(text ...)` artwork.
Native currently flattens every curve into dense polylines before OCCT ever
sees them; build123d hands OCCT exact curves. The result: native renders of
real artwork are an order of magnitude slower (real lineart SVGs push the
render past the 60 s MCP timeout) and geometrically worse (12-segment faceted
arcs in STEP/STL where build123d exports smooth splines).

Follow-up to `svg-native-artwork-parity`, which fixed *which* artwork native
accepts. This change fixes *how* the accepted artwork is represented.

## Problem (evidence)

How the reference backends actually do it (investigated, not assumed):

- **build123d `import_svg`** → `ocpsvg` builds OCCT edges directly from the
  path data: lines become `GC_MakeSegment`, quadratic/cubic Béziers become
  `Geom_BezierCurve`, arcs are converted upstream by the SVG parser into
  cubics. No flattening. A 20-segment outline is a wire with 20 edges.
- **build123d `Text`** → OCCT `Font_BRepFont`, which builds glyph wires from
  the font's Bézier outlines. Also exact.

What native does today:

- `ecky_cad_host/svg_profile.rs` flattens every cubic into `CURVE_SAMPLES=12`
  and every quad into `QUAD_SAMPLES=8` line segments
  (`sample_cubic`/`sample_quad`), then emits one `polygon` plan node per
  contour. A single glyph ("В", 4.8 mm) becomes an 83-point polygon; real
  lineart SVGs (e.g. bananas-lineart, 11 overlapping paths) become thousands
  of vertices.
- `ecky_cad_host/text_profile.rs` does the same for glyph outlines.
- The C++ soup resolver (`make_faces_from_wire_soup`, and its emitted-shim
  twin) classifies containment **per wire vertex** with
  `BRepClass_FaceClassifier`. Cost is O(wires² × vertices × face edges);
  polyline-dense wires turn icon-sized artwork into minutes of CPU.
- Every downstream OCCT op (MakeFace, prism, booleans, tessellation) chews
  through hundreds of tiny edges instead of a handful of exact curves.
- Fidelity: the flattening is baked into the model — STEP export and STL
  tessellation carry the 12-segment facets, so curved artwork looks visibly
  worse than the build123d render of the same file. Reducing sample counts
  (as was done historically on the build123d side before it moved to exact
  import) only trades quality against speed; it cannot reach parity.

## Variables

- **Goal:** native `(svg ...)` / `(text ...)` renders in the same order of
  time and with the same curve fidelity as build123d for identical input.
- **Artifact model:** SVG path / glyph outline → list of closed contours,
  each a list of exact segments (line | cubic Bézier) → `bezier-path` plan
  wires → OCCT `Geom_BezierCurve` edges → profile faces → extrude.
- **Content format:** usvg path data (usvg already normalizes arcs to
  cubics); TTF glyph outlines (quads elevated to cubics losslessly:
  c1 = p0 + ⅔(q − p0), c2 = p2 + ⅔(q − p2)).
- **Plan format:** existing `bezier-path` op (`CorePathOp::BezierPath`,
  3n+1 point3 control points, z = 0) — already supported by the runner,
  the shim executor, and `backend_capabilities`. Pure-line contours keep
  emitting `polygon` unchanged.
- **Validity checks:** Rust-side clean-path checks (`self_intersects`,
  hole containment, multi-outer split) keep operating on the flattened
  sample points — cheap, and only the *checks* use them; emitted geometry
  is exact.
- **Backend ownership:** `svg_profile.rs` / `text_profile.rs` (segment
  extraction), `direct_occt_normalize.rs` / `direct_occt.rs` (plan
  emission), no C++ changes required (bezier-path + profile already
  consume wires; per-vertex containment cost drops because wire vertices
  are now segment endpoints).
- **Frontend ownership:** none.
- **Export format:** STEP carries real Bézier/BSpline curves; STL smoothness
  now governed by the mesher's angular deflection, same as build123d.
- **Runtime constraints:** pure Rust + existing OCCT ops; deterministic.
- **Backward compatibility:** all-line contours (rectangles, straight
  lineart) emit byte-identical `polygon` plans; only curved contours switch
  representation.

## Decision

Stop flattening curves into the geometry. Mirror ocpsvg:

1. `svg_profile.rs` and `text_profile.rs` return, per contour, the exact
   segment list (line | cubic) alongside the existing flattened points
   (which remain for validity checks only).
2. Plan emission (`normalized_svg_polygon_node` and the text-glyph
   equivalent) converts each contour with at least one curve into a
   `bezier-path` node: consecutive cubics, lines encoded as exact cubics
   with collinear control points at ⅓ and ⅔ span. Contours that are all
   lines keep emitting `polygon`.
3. The wire-soup path emits the same exact wires; the C++ resolver is
   unchanged and gets its O(n²) vertex classification input shrunk from
   flattened-point counts to segment-endpoint counts (an order of
   magnitude fewer for typical artwork).

## Rejected Paths

- **Reduce `CURVE_SAMPLES`/`QUAD_SAMPLES`.** Cheaper but still faceted and
  still polyline-dense for large artwork; the build123d side already
  demonstrated this dead end before moving to exact import.
- **Optimize the resolver only (bbox prechecks, BRepFeat::IsInside).**
  Attacks the symptom; leaves faceted geometry and dense downstream booleans.
- **New plan op for mixed line/cubic wires.** Unnecessary — `bezier-path`
  with degree-3 segments represents lines exactly; no ABI change needed.
- **Rasterize / coarser fit sampling.** Out of scope, quality loss.

## Scope

- Segment extraction in `svg_profile.rs` (usvg segments → line/cubic list,
  fit/transform applied to control points).
- Segment extraction in `text_profile.rs` (TTF outline callbacks → cubics,
  quads elevated exactly).
- Plan emission in `direct_occt_normalize.rs` (svg) and `direct_occt.rs`
  (text glyphs): contour → `bezier-path` | `polygon`.
- Tests: plan-emission unit tests (curved contour ⇒ `bezier-path`, straight
  contour ⇒ `polygon`), live render of curved artwork asserting manifold STL
  and sane wall-clock, existing svg/text suites stay green.

## Out of Scope

- C++ runner/shim changes (ops already exist).
- Stroke-to-ribbon lineart, rasterized SVG, text-on-path.
- build123d/FreeCAD backends (they are the reference; unchanged).
- Arc segments as true `Geom_TrimmedCurve` circles (usvg hands us cubics;
  matching ocpsvg's cubic representation is the parity target).

## Proof Plan

- Unit: curved SVG contour and Cyrillic glyph emit `bezier-path` control
  points (3n+1, z=0) and no >20-point `polygon`; straight-edge SVG emits
  `polygon` identical to today.
- Integration: live runner render of the woodlouse-hotel fixture (text +
  two artwork SVGs) stays green with zero non-manifold edges and completes
  within the existing test budget; STEP export contains BSpline/Bézier
  curve entities for the artwork faces.
- Perf guard: live render of a dense synthetic lineart SVG (many curved
  subpaths) completes within a generous wall-clock bound that the flattened
  pipeline exceeds by an order of magnitude.
