# Tasks: FreeCAD Component Transpiler

Pipeline: `.fcstd` → transpiler → parametric ecky model → `component_extract
--save` → versioned stdlib package → import. Only the transpiler is new;
everything downstream already exists.

## 1. Transpiler core (freecadcmd-driven)

- [ ] 1.1 Rust wrapper that resolves `freecadcmd` (reuse `freecad.rs`
  resolution) and runs the transpile script over a `.fcstd`, capturing emitted
  `.ecky` and diagnostics. Behind a `freecad_transpile` module + a CLI bin
  (`transpile_freecad_to_ecky`) mirroring `render_ecky_ir_native_occt`.
- [ ] 1.2 Feature-tree walk: open doc, order `doc.Objects`, detect root solids
  (not consumed as Base/Tool/Shapes; exclude pure inputs like `Part::Extrusion`).
- [ ] 1.3 Primitive + boolean mapping: Box/Cylinder/Cone/Sphere → primitives;
  Cut/MultiFuse/Common → difference/union/intersection. (Prototype already
  proven on `lego_brick.fcstd`.)
- [ ] 1.4 Fillet/Chamfer/Thickness → `fillet`/`chamfer`/`shell` (uniform; carry
  two-radius edges into `fillet :to-radius`).
- [ ] 1.5 Placement (non-identity) → `translate`/`rotate`/`place` wrappers.
- [ ] 1.6 Unsupported feature → tagged error; no silent partial output.

## 2. Sketches & extrusions

- [ ] 2.1 `Sketcher::SketchObject` geometry → ecky profile: lines→polyline,
  arcs→arc segments, circles→circle; closed-wire detection.
- [ ] 2.2 `Part::Extrusion` of a sketch → `(extrude <profile> height)`.
- [ ] 2.3 Pocket/Pad-equivalents that resolve to extrude+boolean.

## 3. Parametrization (dead numbers → params)

- [ ] 3.1 `Part::Array` (ortho/polar) → `repeat-union` / array op with count +
  interval; bind count to a param where exposed. (This makes grids/rows
  parametric — the key fidelity win.)
- [ ] 3.2 Expression/Spreadsheet bindings: lift referenced cells/aliases into a
  `(params …)` block; translate the bound expression to ecky arithmetic in place
  of the literal. Plain (unbound) literals stay literal.
- [ ] 3.3 Emit a well-formed `(model (params …) (part …))` ready for extract.

## 4. Extract bridge + stdlib population

- [ ] 4.1 Drive `component_extract --save` on the transpiled model; verify the
  params become the component signature and the package persists under
  `component-library/<id>/<version>/`.
- [ ] 4.2 Author `verify` clauses per component (manifold, key dimensions) so it
  self-checks; ensure they travel into the saved component.
- [ ] 4.3 Seed an initial stdlib batch by transpiling a curated set from the
  freecad-library (a fastener, a bracket, a mechanical part) — not hand-written.

## 5. Parity & tests

- [ ] 5.1 Reusable parity harness: render native + the supporting OCCT backend
  (build123d, or FreeCAD via freecadcmd→STEP), measure bbox+volume with
  build123d `import_step`, assert within tolerance. (FreeCAD half already
  validated manually.)
- [ ] 5.2 Round-trip test per seeded component: `.fcstd` → transpile → extract →
  `component_get` → instantiate in a model → compile + render + `verify` green.
- [ ] 5.3 Negative: a BREP-only `.fcstd` (no feature tree) is rejected as a
  component candidate with a clear "import-as-mesh only" diagnostic.

## 6. Coordination

- [ ] 6.1 Do not change the `ecky-package.json` / `ecky-header.json` manifest
  format — it is the shared contract with Phase 3 (`component_import`) and any
  parallel agent working on import.
- [ ] 6.2 Components must satisfy the copy-inline guard (stdlib change 4.4):
  self-contained, no implicit registry reference. `component_extract` already
  produces closed copy-inline output — keep it that way.
