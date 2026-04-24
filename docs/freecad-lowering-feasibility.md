# FreeCAD Lowering Feasibility Analysis

**Date:** 2026-04-20

## Status Update — 2026-04-24

No longer hypothetical. Core FreeCAD lowering path exists and current backend corpus is live.

Public source semantics now:

- authored language stays `ecky`
- canonical authored extension stays `.ecky`
- `geometryBackend=freecad` selects FreeCAD lowering

Landed:

- `freecad_lowering.rs` added
- `ecky_ir::lower_to_freecad(...)` exported
- Ecky render dispatch lowers Ecky source to FreeCAD Python when backend=`freecad`
- runner contract normalized to `_ecky_parts = [(name, shape)]`
- runner registers document objects in adapter, not generated source
- hard-op support landed for:
  - `text`
  - `svg`
  - `import-stl`
- repeat lowering landed for:
  - `repeat-union`
  - `repeat-compound`
  - `repeat-pick`
- scalar/point lowering now handles compiler-emitted scoped `let` / `let*`
- real FreeCAD render proofs landed for Ecky source:
  - canonical cup fixture
  - Thomas body fixture
  - Thomas full ramp fixture

Current coverage:

- primitives
- core 2D/path inputs
- constructive ops:
  - `extrude`
  - `revolve`
  - `loft`
- booleans
- transforms
- arrays/repeats
- align
- clip-box
- plane/location/place

Recent closeout additions on 2026-04-24:

- `circle`
- `profile`
- `bezier-path`
- `offset`
- `rounded-rect`
- `rounded-polygon`
- `taper`
- `twist`
- named arrays:
  - `linear-array`
  - `radial-array`
  - `grid-array`
  - `arc-array`
- `xor`

Current audited backend gap:

- no explicit unsupported node left in the audited FreeCAD corpus
- still treat new op combinations as backend-specific until proven by render/parity

Current runner-backed proof on 2026-04-23:

- `render_model_with_sources_renders_ecky_canonical_cup_via_freecad` — passing
- `render_model_with_sources_renders_ecky_thomas_body_via_freecad` — passing
- `render_model_with_sources_renders_ecky_thomas_ramp_via_freecad` — passing

Current parity corpus status on 2026-04-24:

- `canonical_cup` — EXCELLENT MATCH, `0.30%` volume drift
- `compound_boolean` — EXCELLENT MATCH
- `segment_clip` — EXCELLENT MATCH
- `repeat_segments` — EXCELLENT MATCH, `1.13%` volume drift
- `frame_peg_attach` — EXCELLENT MATCH
- `plane_location_place` — EXCELLENT MATCH
- `path_frame_pose` — EXCELLENT MATCH
- `path_frame_up_pose` — EXCELLENT MATCH
- `thomas_modular_ramp_body` — EXCELLENT MATCH
- `thomas_modular_ramp_grooves` — EXCELLENT MATCH
- `thomas_modular_ramp_teeth` — EXCELLENT MATCH, `0.93%` volume drift
- `thomas_modular_ramp_connectors` — EXCELLENT MATCH, `0.93%` volume drift
- `thomas_modular_ramp` — EXCELLENT MATCH, `0.93%` volume drift

Recent backend fixes behind that board:

- `bspline` endpoint tangents/scalars now passed into FreeCAD interpolate helper
- `revolve` sketch plane normalized to match Ecky/build123d semantics
- `shell` no longer no-ops on cup stack; now uses top planar opening faces before offset fallback
- `clip-box` normalizes reversed ranges
- `place` now resolves `path-frame` / `location` / `plane` origins instead of ignoring non-plane frames
- `place` now applies full frame orientation, not origin-only translation
- `path-frame :up` now hard-fails when parallel to tangent, matching build123d semantics
- parity harness now forces fresh FreeCAD lowering and modern `python3` for `compare_metric.py`

Rest of this doc still useful for effort shape. Binary feasibility question already resolved.

---

## 1. What "FreeCAD lowering" means

Add a new code path: `.ecky` Scheme source → Core IR → **FreeCAD Part/Python** (instead of build123d Python). The output would be executed by the existing `freecad_runner.py` via `freecadcmd`.

```
.ecky source
    │
    ▼
CoreProgram (already exists)
    │
    ▼  ← NEW: freecad_lowering.rs
IrModel → FreeCAD Python string
    │
    ▼
freecad_runner.py (already exists, needs minor patches)
```

---

## 2. Comparison: build123d vs FreeCAD Python API

| Ecky IR node | build123d lowering | FreeCAD equivalent | Difficulty |
|---|---|---|---|
| **box** | `Box(w, d, h, align=...)` | `Part.makeBox(w, d, h, origin)` | Easy — but FreeCAD uses corner origin, not center. Need offset calc. |
| **sphere** | `Sphere(r, align=...)` | `Part.makeSphere(r, center)` | Easy |
| **cylinder** | `Cylinder(r, h, align=...)` | `Part.makeCylinder(r, h, center, dir)` | Easy |
| **cone** | `Cone(br, tr, h, align=...)` | `Part.makeCone(br, tr, h)` | Easy |
| **circle** | `Circle(r)` | `Part.makeCircle(r)` → `Part.Wire` → `Part.Face` | Moderate — need explicit Face wrapping |
| **rectangle** | `Rectangle(w, h)` | Manual: 4 edges → Wire → Face | Moderate |
| **rounded-rect** | `RectangleRounded(w, h, r)` | No built-in. Manual wire with arcs. | **Hard** |
| **polygon** | `Polygon(pts)` → `make_face` | `Part.makePolygon(pts)` → `Part.Face(Wire(...))` | Moderate |
| **extrude** | `extrude(face, amount)` | `face.extrude(Vector(0,0,h))` | Easy |
| **revolve** | `revolve(face, axis, arc)` | `face.revolve(center, axis, angle)` | Easy — different axis semantics |
| **loft** | `loft([sections])` | `Part.makeLoft(wires, solid=True)` | Easy — FreeCAD's loft is straightforward |
| **sweep** | `sweep(face, path=wire)` | `Part.makeSweepSurface(wire, profile)` or `BRepOffsetAPI` | **Hard** — FreeCAD sweep API is fiddly |
| **union/fuse** | `shape.fuse(other)` wrapper | `shape.fuse(other)` | Easy — identical concept |
| **difference/cut** | `shape - other` / `_ecky_cut_many` | `shape.cut(other)` | Easy |
| **intersection** | `shape & other` / `_ecky_common_many` | `shape.common(other)` | Easy |
| **translate** | `Pos(x,y,z) * shape` | `shape.translated(Vector(x,y,z))` | Easy |
| **rotate** | `Rot(rx,ry,rz) * shape` | `shape.rotated(...)` / placement matrix | Moderate — Euler convention |
| **scale** | `shape.scale(f)` | `shape.transformGeometry(matrix)` | Moderate — non-uniform needs matrix |
| **mirror** | `mirror(shape, about=Plane)` | `shape.mirror(point, normal)` | Easy |
| **fillet** | `fillet(edges, r)` | `shape.makeFillet(r, edges)` | **Hard** — edge selection is totally different |
| **chamfer** | `chamfer(edges, r)` | `shape.makeChamfer(r, edges)` | **Hard** — same edge selection problem |
| **shell** | Boolean inner or `offset(openings)` | `shape.makeOffsetShape(t)` | Moderate |
| **offset (2D)** | `offset(sketch, amount)` | `wire.makeOffset2D(amount)` → Face | Moderate |
| **path** | `Polyline(pts)` | `Part.makePolygon(pts)` → Wire | Easy |
| **bezier-path** | `Bezier(pts)` chain | `Part.BezierCurve()` + `.toShape()` | Moderate |
| **bspline** | `Spline(pts)` | `Part.BSplineCurve()` + `.interpolate(pts)` | Moderate |
| **text** | `Text(str, size)` | `Part.makeWireString(str, font, size)` | **Hard** — font path required, wire→face→extrude |
| **svg** | `import_svg(path)` | `importSVG.insert(path, doc)` | **Hard** — FreeCAD SVG import is doc-level |
| **import-stl** | `import_stl(path)` | `Mesh.insert(path, doc)` → Part conversion | Moderate |
| **profile (holes)** | `Face(outer, [holes])` | `Part.Face(outer_wire, [hole_wires])` | Easy — FreeCAD supports this natively |
| **twist** | loft with rotated sections | Same approach — loft rotated wires | Easy (same pattern) |
| **taper** | loft with scaled sections | Same approach | Moderate (non-uniform scale on wire) |
| **linear-array** | for-loop with `Pos * shape` | for-loop with `.translated()` | Easy |
| **radial-array** | for-loop with `Rot * Pos * shape` | for-loop with placement | Easy |
| **place / plane / location / path-frame** | `_ecky_place()` helpers | `Placement(Vector, Rotation)` | Moderate |
| **clip-box** | `common(shape, Box)` | `shape.common(Part.makeBox(...))` | Easy |
| **align keyword** | Compute offset per axis | Same — compute offset | Easy (same math) |
| **wall-pattern** | ❌ Not supported | Same — not trivial in Part API | N/A |

---

## 3. What makes it hard (vs build123d)

### 3.1 FreeCAD has no "algebra mode"

build123d lets you write `Box(10,10,10) - Cylinder(5,20)` directly. FreeCAD's Part API is imperative: you create `Part.makeBox()`, then call `shape.cut(other_shape)`. The lowered code needs explicit variable management for every intermediate result — but **the Linearizer already does this**. The `_vN = ...` pattern maps directly.

### 3.2 Edge selection is fundamentally different

build123d: `shape.edges().sort_by(Axis.Z)[-1]` — topological query on the live BRep.
FreeCAD: `shape.Edges` gives you a flat list. Filtering by axis requires manual geometric queries. The `fillet`/`chamfer` edge selectors (`all`, `top`, `bottom`, `vertical`) need custom Python helpers that iterate `shape.Edges` and classify by direction/position.

This is the **hardest single piece** and accounts for ~30% of the effort.

### 3.3 2D sketch workflow

build123d: Sketches are first-class `Face` objects. `Circle(r)` returns a face.
FreeCAD: You must explicitly create `Wire` → `Part.Face(wire)`. Every 2D primitive needs wrapping. The build123d lowerer already has `_ecky_face()` for this — the FreeCAD version would need `_fc_face(wire)`.

### 3.4 Align keyword

build123d has `Align.MIN/CENTER/MAX`. FreeCAD primitives always start at origin. The lowerer must compute post-creation translations based on the align tuple and the primitive dimensions. Doable but verbose.

### 3.5 Runner contract mismatch

The FreeCAD runner (`freecad_runner.py`) expects the macro to create objects on `App.ActiveDocument`. The build123d runner expects `_ecky_parts = [...]`. You'd need either:
- A new `_ecky_parts`-style runner for FreeCAD (cleanest)
- Or emit `doc.addObject("Part::Feature", ...)` calls (more FreeCAD-native)

The cleanest approach: emit `_ecky_parts = [(name, shape)]` and add a FreeCAD-side adapter in the runner that registers them as document objects.

---

## 4. Effort Estimate

### Reusable from build123d_lowering.rs (~70%)

- `ExprLowerer` structure, `Linearizer`, `PyExpr` tree → directly reusable
- `LoweringScope`, binding resolution → identical
- Numeric/boolean/string expression lowering → identical
- `ParsedCallArgs`, keyword parsing → identical
- `lower_model()` scaffold → identical
- All list materialization, point lowering → identical
- `if`, `let`, `build` handling → identical

### New code needed (~30%)

| Item | Est. LoC | Difficulty |
|------|---------|------------|
| FreeCAD preamble helpers (`_fc_face`, `_fc_fuse`, `_fc_cut`, `_fc_common`, align helpers) | ~120 | Easy |
| Primitive lowering (box, sphere, cylinder, cone) with align | ~100 | Easy |
| 2D sketch lowering (circle, rectangle, rounded-rect, polygon) | ~120 | Moderate |
| Boolean ops (fuse, cut, common, compound) | ~60 | Easy |
| Transform lowering (translate, rotate, scale, mirror) | ~80 | Easy-Moderate |
| Extrude, revolve, loft, sweep, twist, taper | ~150 | Moderate |
| Fillet/chamfer with edge selection helpers | ~150 | **Hard** |
| Shell lowering | ~60 | Easy (reuse plan_shell_target) |
| Path ops (polyline, bezier, bspline) | ~80 | Moderate |
| Profile (outer + holes) | ~40 | Easy |
| Text, SVG, import-stl | ~60 | Hard (text especially) |
| Arrays (linear, radial, grid, arc, repeat-*) | ~100 | Easy (same loop patterns) |
| Place/plane/location/path-frame | ~80 | Moderate |
| Clip-box, offset | ~40 | Easy |
| Runner adapter (FreeCAD runner patch for `_ecky_parts`) | ~30 | Easy |
| Integration in `ecky_ir/mod.rs` + `services/render.rs` | ~30 | Easy |
| Tests (mirroring build123d_lowering_tests.rs) | ~400 | Proportional |
| **Total** | **~1,700** | |

### Calendar time

| Scenario | Time | Notes |
|----------|------|-------|
| Core primitives + booleans + transforms + extrude/loft (covers 80% of models) | **3-4 days** | Skip fillet/chamfer/text/svg initially |
| Full parity with build123d lowerer | **6-8 days** | Fillet/chamfer edge selection is the long pole |
| With tests + CI integration | **8-10 days** | Matching the 50+ build123d lowering tests |

---

## 5. Verdict: Definitely doable

**Difficulty: Medium.** Most of the architecture is already solved — the `ExprLowerer` → `PyExpr` → `Linearizer` pipeline is backend-agnostic. The FreeCAD lowerer would be a new `freecad_lowering.rs` file of ~1,200-1,500 LoC (vs 3,500 for build123d) because FreeCAD's Part API is simpler for basic operations.

**The hard parts are:**
1. **Edge selection for fillet/chamfer** (~30% of the new effort)
2. **2D sketch Face wrapping** (annoying but straightforward)
3. **Text rendering** (FreeCAD font API is clunky)

**The easy parts are:**
1. All primitive creation — FreeCAD's `Part.make*` is simple
2. Boolean ops — `shape.fuse/cut/common` is cleaner than build123d
3. Transforms — `shape.translated/rotated` is direct
4. Arrays, repeats — same loop patterns, just different transform syntax
5. The entire lowering scaffold — `ExprLowerer`, `Linearizer`, scope, etc. can be shared or cloned

**Recommended approach:**
1. Create `freecad_lowering.rs` by forking `build123d_lowering.rs`
2. Replace `b123d_preamble()` with `freecad_preamble()` (FreeCAD-specific helpers)
3. Replace each `match` arm in `lower_geom_expr` with FreeCAD Python calls
4. Start with primitives + booleans + transforms + extrude (covers most real models)
5. Add fillet/chamfer/sweep last — they're the hardest and least critical
6. Add `GeometryBackend::Freecad` dispatch for `.ecky` source in `services/render.rs`

**Not recommended: sharing code between lowerers.** The two backends have enough API-level divergence (align semantics, Face wrapping, edge selection, transform syntax) that trying to abstract them would be more complex than two focused ~1,500 LoC files. The `IrModel` layer already provides the shared abstraction.
