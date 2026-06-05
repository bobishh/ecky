# Design: FreeCAD Component Transpiler

## 1. Existing infrastructure (reuse, do not rebuild)

Verified present in the codebase (from "component-unification T5"):

- **Storage + manifest**: `component_package_runtime.rs` —
  `component-library/<package_id>/<version>/` holding `ecky-package.json`
  (`ComponentPackage`) + `ecky-header.json`. Archive read/write, install,
  version-keyed resolve (`resolve_installed_component_source`,
  `list_installed_component_package_headers`). → covers stdlib storage,
  manifest, and version pinning.
- **Extract**: `component_extract::extract_component` lifts a part subtree into
  a closed copy-inline `define-component`; referenced model params become the
  signature; scalar outer `let` bindings become defaults; other free references
  are reported as blockers. `--save` writes it to the library.
- **Discovery**: `component_search` (header-only) + `component_get` (full source)
  over the library (`search_extracted_components` / `read_extracted_component`).
- **Import (Phase 3 of stdlib change)**: shared import path + MCP
  `component_import` + workbench panel; copy-inline only (guard 4.4).

The transpiler plugs into the **front** of this chain. Nothing downstream
changes.

## 2. Sourcing pipeline

```
FreeCAD .fcstd  ──transpiler──▶  parametric ecky model  ──component_extract --save──▶
   define-component (versioned package)  ──search/get──▶  ──component_import──▶  model
```

## 3. Transpiler

Runs under `freecadcmd` (FreeCAD's Python is the only API to a `.fcstd` feature
tree). A small Rust wrapper invokes it (mirroring `freecad.rs::resolve_freecad_path`
+ the freecad render harness) and captures emitted `.ecky` on stdout.

### 3.1 Walk
- Open the document; enumerate `doc.Objects` in dependency order.
- Determine **root solids**: objects not consumed as `Base`/`Tool`/`Shapes` of
  another object (and not pure inputs like `Part::Extrusion` feeding a `Cut`).
- Emit one `part` per root (usually one).

### 3.2 Feature → ecky mapping
Recursive over each feature's inputs:

| FreeCAD | Ecky emission |
| --- | --- |
| `Part::Box(L,W,H)` | `(box L W H)` |
| `Part::Cylinder(R,H)` | `(cylinder R H)` |
| `Part::Cone(R1,R2,H)` | `(cone R1 R2 H)` |
| `Part::Sphere(R)` | `(sphere R)` |
| `Part::Cut(Base,Tool)` | `(difference <Base> <Tool>)` |
| `Part::MultiFuse(Shapes…)` / `Fuse` | `(union <Shapes…>)` |
| `Part::Common` | `(intersection …)` |
| `Part::Fillet(Base, edges[r])` | `(fillet r <Base> :edges …)` (uniform); `(fillet r1 :to-radius r2 …)` when the edge carries two radii |
| `Part::Chamfer` | `(chamfer …)` |
| `Part::Thickness(faces, value)` | `(shell value <Base> :faces …)` |
| `Sketcher::SketchObject` | `(profile …)` / `(polygon …)` from sketch geometry (lines→polyline, arcs→arc, circles→circle) |
| `Part::Extrusion(Base, dir)` | `(extrude <Base> height)` |
| `Part::Array` (ortho/polar) | `(repeat-union i N <child translated by interval>)` or radial-array |
| `Placement` (non-identity) | wrap child in `(translate …)` / `(rotate …)` / `(place …)` |

Unsupported feature types emit a clearly tagged `; UNSUPPORTED <TypeId>` marker
and fail the transpile (no silent partial output for stdlib candidates).

### 3.3 Parametrization
The difference between "dead numbers" and a real component:

- **Arrays** carry their count + interval → emit `repeat-union` with the count
  bound to a param (e.g. `cols`, `rows`) when the count is exposed; derive
  positions from the interval. This alone makes grid/row features parametric.
- **Named expressions / spreadsheet**: FreeCAD lets dimensions reference
  `Spreadsheet` cells or aliased expressions. Where a feature property is bound
  to an expression, lift the referenced cell/alias into a `(params …)` entry and
  emit the expression (translated to ecky arithmetic) in place of the literal.
- **Plain literals** with no expression binding stay literal (a fixed
  dimension), which is correct — not everything is a parameter.

Result: a `(model (params …) (part …))` where params come from arrays +
expression-bound dimensions, ready for `component_extract` to lift into a
`define-component` signature.

### 3.4 Extract bridge
The transpiler does **not** emit `define-component` directly. It emits the
parametric model; the caller runs `component_extract --save` (existing tool),
which turns the model's params into the component signature and persists a
closed package. This reuses signature inference + closure-checking already
written and tested.

## 4. Parity & verification

Per the parity-target principle (proposal): each translated component is
rendered native and on the OCCT backend that supports its features, and
bbox+volume compared within tolerance. The reusable harness:

- native: `render_ecky_ir_native_occt` → `model.step`;
- build123d: lower + run (or import the native STEP) and measure with
  `import_step(...).bounding_box()/.volume`;
- FreeCAD: lower → `freecadcmd` → `exportStep` → measure with the same
  build123d `import_step` tool (backend-agnostic measurement).

Each shipped component also carries authored `verify` clauses (stud pitch,
manifold, wall thickness…) so it self-checks on import.

## 5. Where components plug in (consumption)

- **Instantiation**: `(lego-brick :cols 6 :rows 2)` — copy-inline call, params →
  keyword args.
- **Import (Phase 3)**: agent (`component_import`) or workbench panel inserts the
  component copy-inline; version pinned.
- **Assemblies**: `place`/`translate`/`rotate` + boolean — a baseplate is a
  `repeat`/`place` of brick instances; a bolt is `(difference body (hex-bolt …))`.
- **Nesting**: components instantiate components; copy-inline keeps each closed.

## 6. Alternatives considered

- **Hand-author components** (current stdlib-change wording): rejected — lossy,
  slow, and not grounded in real parts.
- **Mesh/STEP import as the stdlib unit**: rejected — frozen, non-parametric;
  retained only for naked-FreeCAD mode.
- **Generic OCCT STEP → ecky reverse engineering**: infeasible — STEP is dead
  BREP with no parametric history.
