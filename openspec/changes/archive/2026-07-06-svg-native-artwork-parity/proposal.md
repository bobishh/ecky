# Proposal: SVG Native Artwork Parity

## Intent

Close the rendering-parity gap that keeps the bundled build123d/FreeCAD Python
runtimes a hard requirement for real-world SVG artwork. Today the Direct OCCT
(Ecky Native) backend rejects lineart, evenodd icons, and multi-contour artwork
that the Python-backed `import_svg` / `importSVG` paths accept. This change makes
native SVG ingestion delegate region resolution to OCCT the same way the Python
path does, so Direct OCCT becomes a complete replacement.

This is the SVG scope of the `native-occt-runtime` umbrella, split out because it
is independently designable and is the concrete blocker for dropping the Python
CAD dependency.

## Problem (evidence)

`backend_capabilities.rs` marks `svg` unsupported for native render with
"requires svg-profile preprocessing before native render". The preprocessor is
`ecky_cad_host::svg_profile::parse_svg_profile`. It enforces, per contour:

- closed paths only (`reject_open_contour`),
- no self-intersection (`self_intersects`, O(n²)),
- a single outer loop in the first slice (`reject_multi_outer_first_slice`).

Real artwork fails one or more of these:

- Lineart (e.g. `bananas-lineart.svg`, 11 `<path>` with overlapping strokes) —
  self-intersecting and often open.
- Flat icons with `fill-rule:evenodd` and multiple subpaths in one `<path>`
  (e.g. `carrot-vegetable-icon.svg`, 3 `M` subpaths, 0 `Z`) — self-intersecting
  and multi-outer.
- Compound icons (several disjoint filled shapes) — multiple outer loops.

## How build123d / FreeCAD actually do it (investigated, not assumed)

The Python path does **not** implement its own self-intersection or arrangement
solver. It delegates to OCCT:

- **build123d `import_svg`** → `ocpsvg.import_svg_document` → `faces_from_wire_soup`
  (`ocpsvg/ocp.py:145`). Per wire it calls `BRepBuilderAPI_MakeFace(wire, True)`
  (the `True` = planar face), which lets OCCT itself split a self-intersecting
  wire into valid planar regions under even-odd/nonzero. Orientation is repaired
  via `ShapeFix_Face(...).FixOrientation()`. Hole/outer nesting is then resolved
  by `BRepFeat.IsInside_s` and parity-of-depth (odd depth = inner ring). A wire
  that cannot become a face is swallowed by an `except InvalidWiresForFace` and
  logged as a warning — it does not fail the whole render.
- **FreeCAD** (`freecad_lowering.rs:3970`) runs `importSVG.insert` then wraps each
  result in `Part.Face(...)` inside try/except, finally `Part.makeCompound(...)`
  of whatever survived.

So the parity target is: **hand the raw wire soup to OCCT's planar face builder
and let OCCT resolve regions, exactly like the Python path.** Ecky's current
Rust-side `self_intersects` / single-outer / closed-only checks pre-empt work
that OCCT already does and is willing to do.

## Variables

- **Goal:** native SVG render parity for artwork; make Direct OCCT a complete
  replacement so build123d/FreeCAD Python runtimes are droppable for SVG.
- **Artifact model:** SVG → raw wire soup (filled paths) → OCCT planar face(s)
  with resolved holes → extrude, in pure Rust + the same OCCT the Python path
  already uses.
- **Content format handled:** filled paths (`fill-rule` nonzero/evenodd),
  multi-subpath `<path>`, multiple disjoint filled contours, closed or
  stroke-ribboned contours.
- **Storage:** existing SVG files on disk (no change).
- **Routing path:** `(svg path w h fit)` op in Ecky IR → Direct OCCT planner →
  SVG wire extraction → OCCT `BRepBuilderAPI_MakeFace` region resolution →
  extrude. build123d/FreeCAD keep their existing tolerant Python paths unchanged.
- **Backend ownership:** `ecky_cad_host/svg_profile.rs` (relax), Direct OCCT
  planner/normalizer (consume wire soup → OCCT faces).
- **Frontend ownership:** none (transparent to UI).
- **Editing model:** none.
- **Testing surface:** `svg_profile` unit tests; Direct OCCT integration render
  of the three artwork classes vs. the build123d reference output.
- **Export format:** STEP/STL from a compound of OCCT-resolved faces.
- **Runtime constraints:** pure Rust + OCCT; no Python; deterministic output.
- **Backward compatibility:** existing clean single-outer-loop profiles keep
  their exact current geometry (the OCCT fast path yields the same face).

## Decision

Stop rejecting artwork pre-emptively. Mirror the build123d/ocpsvg approach:

1. **Extract a wire soup, not a sanitized profile.** `parse_svg_profile` keeps
   returning clean loops for the existing fast path, but gains a tolerant mode
   that yields all filled-path wires (closed and, where applicable, stroked)
   without the Rust-side `self_intersects` / single-outer / closed-only guards.
2. **Delegate region resolution to OCCT.** For native render, hand the wire soup
   to OCCT's planar face builder (`BRepBuilderAPI_MakeFace` with planar = true),
   which resolves self-intersections and even-odd/nonzero regions the same way it
   does for the Python path. Repair with `ShapeFix_Face` / `FixOrientation` and
   resolve hole nesting by containment + parity-of-depth (the ocpsvg algorithm).
3. **Soft-fail on bad wires.** A wire that OCCT cannot turn into a face is
   dropped with a warning, not a hard render failure — matching ocpsvg's
   `except InvalidWiresForFace`. The render succeeds with whatever faces OCCT
   produced.
4. **Open contours:** a filled path that is open is treated as ocpsvg treats it
   — handed to OCCT; if it yields no face, it is dropped with a warning. A
   stroke-only path (no fill) is not in this slice (lineart-as-ribbon is a
   follow-up) — stroke handling stays explicitly out of scope below.
5. **Update capability messaging.** `backend_capabilities` moves native `svg`
   from unsupported to conditionally supported: clean profiles take the existing
   fast path; artwork goes through the OCCT region resolver.

## Rejected Paths

- **Hand-roll a 2D arrangement solver in Rust** (the original proposal draft).
  Rejected after investigation: build123d/ocpsvg do not do this; they rely on
  OCCT's planar face builder. Reimplementing arrangement would diverge from the
  reference behavior we are trying to match.
- **Naively drop `self_intersects` with no downstream resolver.** Rejected: would
  feed OCCT a self-intersecting wire without the MakeFace region step that makes
  it valid. The fix is delegation to `BRepBuilderAPI_MakeFace`, not deletion.
- **Keep routing artwork through Python.** Rejected: preserves the exact
  dependency this change exists to remove.
- **Treat SVG as raster heightfield.** Rejected (out of scope, already excluded
  by `native-occt-runtime`): lithophane/heightmap is a separate path.

## Scope

- Relax `parse_svg_profile` (or add a sibling tolerant ingestion) to emit a wire
  soup without the Rust-side self-intersection / single-outer / closed-only
  rejections.
- Add OCCT planar-face region resolution to the Direct OCCT path, mirroring
  `faces_from_wire_soup` (`BRepBuilderAPI_MakeFace` planar + `ShapeFix_Face` +
  containment/parity nesting).
- Soft-fail individual unfaceable wires with a warning.
- Support compound faces (multiple outer loops) and even-odd/nonzero fills.
- Update `backend_capabilities` messaging for native `svg`.
- Unit + integration tests for lineart, evenodd icon, compound icon, asserting
  parity (within OCCT tolerance) with the build123d output.

## Out of Scope

- Stroke-to-ribbon handling for stroke-only lineart (follow-up change).
- SVG raster/lithophane/heightmap rendering.
- SVG filter effects, gradients, clipping/masks beyond what `usvg` already
  rasterizes away.
- Text-on-path.
- Changing build123d/FreeCAD SVG handling (they stay as-is).
- Removing the Python CAD runtimes (owned by `native-occt-runtime`).

## Dependencies

- Depends on `native-occt-runtime` for the Direct OCCT runtime/ABI that consumes
  the resolved face compound.
- `usvg` (already a dependency) for path extraction.

## Proof Plan

- Unit: feed the three representative SVG classes (lineart, evenodd multi-subpath,
  compound) through the tolerant ingestion and assert the wire soup reaches OCCT
  face resolution without pre-emptive rejection.
- Integration: render `bananas-lineart.svg` and `carrot-vegetable-icon.svg` via
  Direct OCCT and via build123d; assert STEP/STL topology matches within OCCT
  tolerance (same face/hole counts and bounds), with no Python fallback.
- Regression: existing `svg_profile` tests stay green; clean single-outer-loop
  profiles produce the same loops/face as today via the fast path.
