# Design: SVG Native Artwork Parity

## Reference behavior (build123d / ocpsvg), from source

The Python path Ecky already uses (`build123d.importers.import_svg` →
`ocpsvg.import_svg_document`) does **not** implement its own self-intersection or
arrangement solver. It delegates to OCCT. The relevant chain:

`ocpsvg.svg.faces_from_svg_path(path)` →
`ocpsvg.ocp.faces_from_wire_soup(wires)`:

```python
def faces_from_wire_soup(wires):
    wires = list(wires)
    # coplanarity guard only
    if not are_wires_coplanar(wires):
        raise InvalidWiresForFace("wires are not coplanar")

    def fix_wires():
        for wire in wires:
            # BRepBuilderAPI_MakeFace(wire, True) -> planar face.
            # OCCT itself splits a self-intersecting wire into regions.
            yield face_outer_wire(face_from_wires(wire))
    faces = [BRepBuilderAPI_MakeFace(wire, True).Face() for wire in fix_wires()]

    if len(faces) < 2:
        yield from map(ensure_face_normal_up, faces); return

    # hole/outer nesting by containment + parity of depth (even-odd fill)
    included_in = {i: {j for j ... if BRepFeat.IsInside_s(face_i, face_j)} ...}
    for i in range(len(faces)):
        ancestors = included_in.get(i, set())
        if len(ancestors) % 2:        # odd depth -> inner ring
            ...append as inner wire of nearest even-depth ancestor
        else:                         # even depth -> outer ring
            ...emit face_from_wires(outer, inners)
```

`face_from_wires` wraps `BRepBuilderAPI_MakeFace(outer, True)` + adds inner wires,
then `ShapeFix_Face(...).FixOrientation()` to repair orientation.

In `import_svg_document`, a wire that raises `InvalidWiresForFace` is caught and
logged (`"filled shape could not be converted to face"`) — it does not fail the
whole import.

**Implication for native parity:** Ecky's current Rust-side
`self_intersects` / `reject_multi_outer_first_slice` / `reject_open_contour` in
`ecky_cad_host::svg_profile` are stricter than the reference path. They pre-empt
work OCCT already does. The native fix is to mirror ocpsvg: hand the wire soup to
OCCT's planar face builder and let OCCT resolve regions.

## Architecture

```
(svg path w h fit)
   │
   ▼  (Ecky IR, no change)
Direct OCCT planner
   │
   ▼
svg_profile::extract_wire_soup(svg_text, fit)        ← NEW tolerant path
   │   - usvg path extraction (existing)
   │   - fill-rule nonzero/evenodd captured per <path>
   │   - NO Rust self_intersect / single-outer / closed-only rejection
   │   - returns Vec<(wires, fill_rule)>
   ▼
direct_occt native: resolve_faces(wire_soup)         ← NEW
   │   - for each wire: OCCT BRepBuilderAPI_MakeFace(wire, planar=true)
   │   - ShapeFix_Face / FixOrientation
   │   - nesting by BRepFeat IsInside + parity of depth (port of ocpsvg algo)
   │   - unfaceable wire -> warn + skip (do not abort)
   ▼
compound of faces -> extrude -> STEP/STL
```

The existing `parse_svg_profile` (clean single-outer-loop fast path) stays. It is
the path used when the artwork is already a clean profile and by any caller that
wants sanitized loops. The new tolerant path is selected by the Direct OCCT
renderer for the `(svg ...)` op.

## Fill-rule handling

- Capture `fill-rule` per `<path>` from usvg (`nonzero` default, `evenodd`).
- OCCT's `BRepBuilderAPI_MakeFace` resolves a single self-intersecting wire under
  even-odd. ocpsvg additionally relies on nesting-by-containment+parity across
  multiple wires, which implements even-odd across disjoint subpaths. We port
  that nesting step verbatim (it is ~30 lines) so multi-subpath even-odd behaves
  like the reference.
- `nonzero` across multiple wires: ocpsvg does not special-case it; neither do
  we. (Documented limitation, matches parity target.)

## Open contours

A filled but open path: ocpsvg hands it to `BRepBuilderAPI_MakeFace`, which may or
may not produce a face; failure is caught and logged. We do the same — no special
Rust handling. A stroke-only path with no fill produces no face today and is out
of scope (stroke ribboning is a follow-up change).

## Edge cases & determinism

- Coplanarity is guaranteed (all SVG wires are planar in XY); mirror the
  `are_wires_coplanar` guard defensively.
- `ShapeFix_Face` can mutate geometry slightly; this matches the Python path, so
  parity holds within OCCT tolerance.
- Face normal orientation: port `ensure_face_normal_up` so extrusion direction is
  consistent with build123d.

## Capability surface

`backend_capabilities` currently: `unsupported("svg", "requires svg-profile
preprocessing before native render")`. After this change, native `svg` becomes
conditionally supported: the Direct OCCT renderer accepts the `(svg ...)` op and
runs the tolerant path. The message is updated to reflect delegation to OCCT.

## Testing strategy

- **Unit** (`svg_profile`): tolerant extraction returns wire soups for the three
  artwork classes without pre-emptive rejection; fill-rule is captured.
- **Unit** (new resolver module): given synthetic wire soups (nested squares,
  self-crossing bowtie, disjoint pair), assert face/hole counts and nesting
  parity match the ocpsvg algorithm.
- **Integration**: render `bananas-lineart.svg` and `carrot-vegetable-icon.svg`
  through Direct OCCT and through build123d; assert topology (face count, hole
  count, bounding box) matches within OCCT tolerance.
- **Regression**: existing `svg_profile` tests unchanged; clean profile fast
  path produces identical loops.
