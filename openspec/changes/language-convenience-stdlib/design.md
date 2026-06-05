# Design: Convenience Ops & Language Standard Library

## Variables

- **Goal:** reduce author effort for common shapes/parts; raise build123d parity
  for convenience geometry from ~50–65% toward ~85%.
- **Artifact model:** two delivery mechanisms — native ops (compiler + backends)
  and stdlib components (authored `.ecky`).
- **Variables:** which items are native ops vs stdlib; the op signatures; the
  parity verification method; stdlib storage/discovery; import surface.
- **Decision:** see "Native op vs stdlib" rule below.
- **Rejected paths:** (a) everything as native ops — too much kernel code and
  ties convenience parts to release cadence; (b) everything as stdlib — torus /
  ellipse / draft cannot be composed from existing ops, so some must be native.
- **Proof plan:** per-op round-trip (parse→IR→emit) + cross-backend STL parity
  (bbox + volume within tessellation tolerance) + native render smoke.

## Native op vs stdlib — the boundary rule

> An item is a **native op** when it is irreducible geometry that cannot be
> cleanly composed from existing ops, or needs exact-kernel construction
> (curved surfaces, draft, fillet variants, threads). It is an **stdlib
> component** when it is a parametric assembly of existing ops.

Applying it:

| Item | Mechanism | Why |
| --- | --- | --- |
| `torus`, `ellipse`, `wedge` | native primitive | irreducible / curved |
| `regular-polygon`, `slot`, `trapezoid` | native primitive | exact 2D profile, used everywhere |
| `draft`, `rib`, `groove`, variable `fillet` | native feature | needs face/edge selection + kernel ops |
| `thread` | native feature | thin wrapper composing `helical-ridge` + a core cylinder with correct pitch/clearance defaults |
| hex bolt, screw, nut, washer, gear, knurl, clip, boss, vent | stdlib component | composable from the above |

## Per-op recipe (mirrors the `helical-ridge` addition)

For each native op (verified end-to-end on `regular-polygon` — these are ALL the
touchpoints; missing any one fails at a different layer):

1. **Surface binding**: add the op name to `ecky_scheme/cad.rs` `exports` — it is
   auto-bound in Steel as `(define (name . args) ...)`. (A `Custom` op needs no
   compiler dispatch arm; the `_ => Custom(name)` fallback handles it.)
2. **Value kind**: add the op to `infer_value_kind` in `compiler.rs` with its
   result kind (`Sketch` for 2D profiles, etc.). **Gotcha:** `Custom` ops
   default to `Solid`; a 2D op left out here fails `extrude` with
   "expected 2D sketch, got solid".
3. **Native normalizer allowlist**: add the op to the `Custom` match in
   `direct_occt_normalize.rs` or it errors "normalizer does not support custom
   operation".
4. **Native expansion**: add a dispatch arm + `expand_*_node` in
   `direct_occt.rs` that rewrites the op into existing primitives/ops.
5. **Lowerings**: `build123d_lowering` + `freecad_lowering` — a python helper in
   each preamble (same formula) + a dispatch arm. (`ecky check` exercises the
   build123d path only.)
6. **Parity by construction**: put any shared math (vertex/curve formula) in ONE
   `ecky_core_ir` function the native expansion calls, and mirror it in the two
   python helpers; the parity test guards drift.
7. **Tests**: a native planner test (no runtime) + a lowering test, plus the
   bbox/volume **parity test** against build123d.

## The parity gate (non-negotiable)

The `helical-ridge` work proved native and an interop backend can silently
diverge (faceted polyline vs true helix; an empty boolean that vanishes
geometry). So every convenience op carries a **parity test**: render the same
model on native and on build123d, assert equal bounding box and volume within a
tessellation tolerance. An op is not "done" until parity holds. Reference
implementation for a backend is build123d unless noted.

## stdlib mechanism

- Storage: `stdlib/` of versioned `.ecky` files, one `define-component` per
  file, each self-contained and carrying `verify` clauses.
- Discovery: a manifest (name, summary, params, tags, version) consumed by the
  existing `component_search` surface; bodies fetched by `component_get`.
- Loading: import = copy-inline the component source into the model and
  instantiate it (no implicit registry link), matching the existing
  component-library contract.
- Versioning: stdlib entries are pinned by version in the manifest so a model's
  reproducibility does not silently change when the stdlib updates.

## Open questions (for review)

1. ~~Slot variants: one obround now or the full build123d set?~~
   **Resolved:** ship the **full set** — `slot-center-to-center`, `slot-overall`,
   `slot-arc` (curved spine), and `slot-center-point`. They share one obround
   profile builder; the variants differ only in how endpoints/length are
   specified, so the extra cost over a single slot is small.
2. ~~`thread` defaults: ISO metric table or pure parametric?~~
   **Resolved:** **both**, layered. The parametric core (`pitch` / `depth` /
   `clearance` / `length` / handedness / male|female) is the foundation. An ISO
   metric designation (`M3`, `M4`, `M5`, …) is **decoded** via a coarse-pitch
   lookup table into those real parametric values — it is pure sugar, no
   separate code path. `(thread :iso "M4" :length 10)` expands to the same
   parametric call as writing the resolved pitch/diameter by hand. Unknown or
   out-of-table designations fail with a clear diagnostic.
3. ~~Import surface home: MCP tool only, or also a workbench panel?~~
   **Resolved:** both. The import action must be usable by a human directly, not
   only via the agent. Ship an MCP `component_import` tool AND a workbench panel
   (search stdlib/library → click → inserts copy-inlined source). The panel and
   the tool call the same underlying import path so behaviour is identical
   whichever way it is invoked.
