# Proposal: Convenience Ops & Language Standard Library

## Intent

Make Ecky faster to build real parts in by closing the convenience gap with
build123d/FreeCAD on two fronts:

1. **Convenience ops** — a batch of high-value geometry primitives and features
   that today force authors to hand-compose from low-level ops (or are simply
   impossible): `torus`, `ellipse`, `regular-polygon`, `slot`, `trapezoid`,
   `wedge`, plus the feature ops `draft`, `rib`/`groove`, variable-radius
   `fillet`, and a real `thread` built over `helical-ridge`.
2. **Language standard library (stdlib)** — a shipped, versioned set of
   parametric parts authored in `.ecky` as `define-component` (fasteners,
   gears, knurls, snap-fit clips, screw bosses, vents…), loadable into any model
   without re-deriving them, plus an **import** surface to pull a stdlib or
   user-library component into the current model.

The current op surface (~52 geometry ops) covers the modeling spine well, but
authors spend time re-deriving shapes that build123d hands over as one call. A
hex profile is `regular-polygon`; a mounting slot is `slot`; a bolt is an stdlib
component. Closing this is the cheapest large win for authoring speed — each
item is the same shape of work as the `helical-ridge` addition: one
`CoreOperation` (or one `.ecky` component) plus parity across backends.

This change is deliberately split from any sketch-mode work; the orthographic
sketch surface is out of scope here.

## Scope

### Convenience ops (Phase 1 — priority)

- Add convenience **primitives** to Core IR + surface grammar: `torus`,
  `ellipse`, `regular-polygon`, the **full slot set** (`slot-center-to-center`,
  `slot-overall`, `slot-arc`, `slot-center-point`), `trapezoid`, `wedge`.
- Add convenience **feature ops**: `draft` (taper selected faces by angle),
  `rib`, `groove`, variable-radius `fillet` (per-edge radius list), and
  `thread` — a parametric core (pitch/depth/clearance/length/handedness/
  male|female) wrapping `helical-ridge`, with an ISO metric designation
  (`M3`/`M4`/…) decoded to those parametric values as sugar.
- Every new op MUST lower with **geometry parity across all three backends**
  (native OCCT, build123d, FreeCAD) — identical solid, verified by STL
  bounding-box / volume comparison, not just "renders". (See the helicoid
  regression: native and build123d must agree.)
- Each op ships with: surface parse, Core IR node, `verify_core_program`
  type/arity checks, the three lowerings, a native runner expansion where the
  op needs exact construction, and round-trip + parity tests.

### Language standard library (Phase 2)

- Define an stdlib mechanism: a shipped directory of versioned `.ecky`
  `define-component` definitions, discoverable and loadable by name.
- Ship an initial catalogue: fasteners (hex bolt, socket-head screw, nut,
  washer), mechanical (spur gear, knurled grip, snap-fit clip), enclosure
  helpers (screw boss, rounded shell, vent grille).
- Each stdlib component is closed (copy-inlineable) and carries its own
  `verify` clauses so reuse includes proof.

### Component import (Phase 3)

- Add an import surface — usable both by the agent (MCP `component_import`
  tool) and by a human (workbench panel: search stdlib + user-library → click →
  insert) — that inlines a chosen component into the active model as
  instantiated `define-component` source (reusing `component_search`/
  `component_get`; copy-inline, no hidden registry reference). The panel and the
  tool share one import path.

## Out of scope

- Sketch-mode / orthographic constraint work.
- A 2D constraint solver.
- New import file formats (STEP/DXF/IGES in) — tracked separately.
