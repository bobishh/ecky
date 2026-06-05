# Proposal: FreeCAD Component Transpiler → Parametric Ecky Stdlib

## Intent

Source the Ecky standard library by **translating real parametric FreeCAD
parts into parametric Ecky**, instead of hand-authoring `define-component`
files. We already have a FreeCAD component library available (the local
`freecad-library` roots reachable via the `freecad_library_*` MCP tools) and a
complete component subsystem (extract → versioned library → search/get →
import). The only missing piece is a **transpiler** that reads a FreeCAD
document's parametric feature tree and emits parametric `.ecky`.

This re-scopes Phase 2 of `language-convenience-stdlib`. That change currently
asks us to (a) "define stdlib storage + manifest format" — which **already
exists** (`component-library/<package_id>/<version>/` with `ecky-package.json` +
`ecky-header.json`, version pinning, install) — and (b) "ship N components"
without saying where they come from, which in practice means hand-coding them.
Hand-coding loses fidelity (a hand-simplified LEGO brick dropped the hollow
underside + anti-stud tubes that the real `.fcstd` encodes) and is exactly the
busywork we want to avoid.

## Why translate, not import-as-mesh

The existing `freecad_library_import` brings a part in as a **dead mesh/STEP**
(`imported-mesh-*`): frozen geometry you can only place, never re-fit. A
parametric component can be re-fit to its context on import (a 2x4 brick → 6x2,
an M8 bolt → M6, a bracket → wider). That adaptability is the whole point of a
stdlib component. Dead-mesh import stays only for "naked FreeCAD mode".

## Why this is feasible

FreeCAD library parts carry real parametric feature trees that map almost 1:1
onto Ecky ops (verified by introspecting `doc.Objects` on real parts):

| FreeCAD feature | Ecky op |
| --- | --- |
| `Part::Box` / `Cylinder` / `Cone` / `Sphere` | `box` / `cylinder` / `cone` / `sphere` |
| `Part::Cut` / `Part::MultiFuse` | `difference` / `union` |
| `Part::Fillet` / `Part::Chamfer` | `fillet` / `chamfer` |
| `Part::Thickness` | `shell` |
| `Sketcher::SketchObject` + `Part::Extrusion` | `profile`/`polygon` + `extrude` |
| `Part::FeaturePython` "Array" | `repeat-union` / array op |

The Array feature is the key to staying parametric: it already encodes the loop
(e.g. the LEGO studs were a `Part::Array`), so translating it to `repeat-union`
yields parametric geometry **for free** rather than N hardcoded copies.

## Approach

```
FreeCAD .fcstd (parametric feature tree)
  └─[TRANSPILER]→ parametric ecky model (params + part)          ← the only new code
       └─[component_extract --save]→ define-component in library  ← exists today
            └─ versioned ecky-package.json / pinning              ← exists today
                 └─[component_search / component_get]→ discovery   ← exists today
                      └─[component_import]→ copy-inline into model ← Phase 3 of stdlib change
```

`component_extract` is the bridge: the transpiler only has to emit a parametric
**model** (a `(params …)` block + one `part`); `component_extract` already lifts
the part and turns referenced model params into the component signature, then
saves a closed copy-inline `define-component`. No new storage/manifest work.

## Parity target principle

A feature's parity target is **whichever OCCT backend supports it**, not always
build123d. Native and FreeCAD are both OCCT; build123d is OCP (also OCCT) but
exposes a narrower API. So:

- features build123d supports → verify native↔build123d;
- OCCT-only features (e.g. tapered fillet) → verify native↔FreeCAD via the
  freecadcmd → STEP → build123d `import_step` measurement harness.

## Two-stage rollout

The transpiler is the **sourcing front-end of the existing component pipeline**,
not a separate track. Components are produced in two stages:

1. **Bulk transpile** — run the transpiler over many freecad-library parts to
   get *draft* parametric components: geometry faithful 1:1, structure correct,
   arrays already loops, expression-bound dims already params.
2. **Curate** — finish each draft into a shippable stdlib component: name params
   meaningfully (`cols`/`rows`/`wall`, not `Spreadsheet.B3`), decide which
   dimensions are actually parameters, add `verify` clauses, tidy the signature,
   then `component_extract --save`.

The transpiler does the heavy structural lifting; the human/agent does the
semantic finishing on top. "Ship components" therefore means *transpile-then-
curate*, never hand-author from scratch.

## Seed, not live dependency

Transpilation is **one-shot**: the `.fcstd` is only a seed. After transpiling,
the emitted `.ecky` becomes canonical — curation happens in Ecky and the FreeCAD
document is no longer needed. We do **not** re-transpile over a curated
component (manual refinements would be lost). If a source part changes, that is
a fresh transpile into a *new component version*, not an overwrite of the
curated one. This keeps stage-2 work durable and the library version-honest.

## Out of scope

- BREP-only `.fcstd` parts with no feature tree cannot be made parametric; they
  remain mesh/STEP imports (naked-FreeCAD mode), not stdlib components.
- Full sketch-constraint solving. The transpiler reads sketch geometry +
  named expressions / spreadsheet cells where present; constraint inference
  beyond that is a follow-up.
