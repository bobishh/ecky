# Ecky Language Improvement Roadmap

**Status:** Trackable proposal  
**Scope:** `.ecky` language surface, Core IR contracts, selector semantics, units/types, component interfaces, and earlier lowerer errors

## Summary

Ecky is already useful as a compact CAD DSL for LLMs and humans because source is small, parametric, and validateable before backend execution. Current weak spots show up when a model needs precise topology edits, reusable component interfaces, unit-safe arithmetic, or targeted repair:

```text
.ecky source
  -> parser
  -> typed Core IR
  -> semantic selectors
  -> capability validation
  -> backend lowering
  -> artifact verification
```

Goal: make common CAD intent explicit in source and fail before backend lowering whenever possible.

## Non-Goals

- Do not expose direct OCCT as a user backend while it remains internal.
- Do not fork `.ecky` into backend-specific languages.
- Do not make LLM-only conventions the contract.
- Do not claim selectors, ports, units, or fillet behavior until manifest plus tests prove them.
- Do not hide raw backend errors behind generic copy.

## Progress Ledger

Status meanings:

- Done: implemented and verified in current app.
- In progress: partial foundation exists; user-visible or backend parity incomplete.
- Next: planned work not yet implemented.
- Pending: blocked behind earlier language/Core IR work.
- Parked: intentionally deferred.

| Status | Area | Problem | Target Evidence |
| --- | --- | --- | --- |
| In progress | Typed Core IR base | Core IR catches many shape/type mistakes, but selectors, units, and component interfaces remain thin. | Existing typechecker stays authoritative; new signatures add source spans and exact expected/actual kinds. |
| Next | Stable entity ids | Later selectors need edges/faces/features that survive lowerer rewrites. | Generated model manifest maps source node id -> feature -> backend face/edge candidates. |
| Next | Face selectors | `fillet` / `chamfer` cannot honestly say "inside rim only" or "top entry bevel only". | Source can select named faces by role, normal, plane, bbox, adjacency, and originating feature. |
| Next | Edge selectors | Edge modifiers are too coarse: all/top/bottom/vertical is not enough. | Source can select edges by adjacent faces, angle, convexity, radius/length, direction, loop membership, and source feature. |
| Next | Selector preview and validation | Bad selectors fail late or silently select wrong geometry. | Compiler/lowerer reports zero-match, ambiguous-match, backend unsupported, and selected-count mismatch before applying modifier. |
| Next | Unit-aware numbers | `12`, `12mm`, and ratios all flow as plain numbers. | Typechecker separates length, angle, ratio, count, bool, point2, point3, vector, plane, and geometry. |
| Next | Unit conversion and defaults | Params lack explicit unit contracts. | Params declare units; UI shows units; backend receives normalized canonical values. |
| Next | Stroke-first authoring | Users need a simpler "line plus thickness plus height" path into CAD VM solids. | `.ecky` can express typed strokes, caps, joins, thickened profiles, sweeps, extrusions, and continuous-body validation targets. |
| Next | Component declaration | Reusable parts lack first-class language shape. | `.ecky` can declare a component with params, ports, keepouts, exports, validation rules, and source version metadata. |
| Next | Port declaration | Dovetail/bolt/socket interfaces live as comments and conventions. | `.ecky` can expose named ports with type, frame, clearance, side, keepout, mate rules, and validation. |
| Next | User-defined port types | Hardcoded port lists will age badly. | Data-defined port schemas with required fields, compatibility predicates, and no arbitrary untrusted code in v1. |
| Next | Mate/assembly source | Assembly is not yet a first-class language object. | `.ecky` can reference components, override params, mate ports, preserve separable parts, or request fuse/cut/mold. |
| Next | Earlier lowerer errors | Some invalid source reaches Steel/lowerer/backend and fails too late. | Parse/type/capability validation catches depth, arity, unsupported op, selector, and backend mismatch before lowering. |
| Next | Lowerer stack guard | Huge/deep generated source can stack overflow in lowering. | Depth/node-count guards return raw structured validation errors, no process crash. |
| Next | Capability manifest hard gate | Guides and lowerers can drift. | Source validation checks selected backend manifest before render; guide tests assert no unsupported language claims. |
| Pending | Feature history graph | Model-to-sketch and selector repair need durable feature provenance. | Manifest stores feature graph and projections; derived sketches stay separate from authoring sketches. |
| Pending | Constraint-level language | Sketch dimensions and constraints exist as source evidence, not full parametric solving. | Constraints are typed, named, and solved or rejected before backend preview/build. |

Recent landed narrow slice:

- `sampled-radial-loft` now exists as an exact-only `.ecky` op for sampled radial section lofts. It lowers on FreeCAD and build123d, participates in build123d plus internal direct-OCCT sampled shell planning, shows in the canonical surface reference only for exact backends, probes internal direct-OCCT first under `EckyRust` before falling forward to build123d, and rejects mixed mesh-only plus exact-only sources early.

## Proposed Language Shape

Syntax below is proposal, not current accepted `.ecky`.

### Named Features

Problem: selectors need stable anchors. Anonymous nested CSG makes "that inner edge" fragile.

```lisp
(feature cage-shell
  :role bottle-wrap
  (difference
    (cylinder out_r height :align '(center center center))
    (cylinder in_r (+ height 10mm) :align '(center center center))))
```

Acceptance:

- feature id is stable in source
- manifest records feature id
- duplicate feature ids fail before lowering
- backend output can report feature provenance when supported

### Face Selectors

Problem: face selection must be exact enough for fillet/chamfer/shell/cut/mate.

```lisp
(select-face cage-shell
  :where (and
    (role bottle-wrap)
    (normal-near +z)
    (within-z 64mm 66mm)))
```

Selector primitives:

- `role`
- `source-feature`
- `normal-near`
- `plane-near`
- `bbox`
- `within-x`, `within-y`, `within-z`
- `area-between`
- `adjacent-to`
- `loop`
- `inner`, `outer`

Validation:

- zero matches: fail with selector text and candidate count
- too many matches: fail unless `:allow-many #t`
- backend unsupported: fail before backend modifier
- selector result must be inspectable in manifest/debug panel

### Edge Selectors

Problem: `fillet` and `chamfer` need precise edge sets.

```lisp
(chamfer 0.8mm
  (edges ledge-base
    :where (and
      (adjacent-face-role bottom-ledge)
      (convex #t)
      (direction-near tangent)
      (not (adjacent-face-role bottle-clearance)))))
```

Selector primitives:

- `adjacent-face`
- `adjacent-face-role`
- `convex`, `concave`
- `angle-between`
- `direction-near`
- `length-between`
- `radius-between`
- `loop-index`
- `source-feature`
- `created-by`

Acceptance:

- can chamfer outer bottom ledge without touching inner bottle hole
- can fillet inner bottle rim without touching outer cage wall
- can bevel female dovetail stop without adding exterior wedge geometry
- selector failure reports raw structured issue, not backend stack trace

### Units And Typed Numbers

Problem: dimensions, ratios, counts, angles, and indexes are all plain numbers.

```lisp
(params
  (length bottle_diameter 74mm :min 60mm :max 90mm)
  (length wall_thickness 4mm :min 2mm :max 6mm)
  (ratio front_opening_width_ratio 0.72 :min 0.55 :max 0.92 :step 0.02)
  (angle draft_angle 45deg)
  (count hex_rows 6 :min 2 :max 12))
```

Rules:

- canonical internal length unit: millimeter
- canonical angle unit: radian or degree, pick one internally and normalize at boundary
- ratios are dimensionless and cannot be added to lengths
- counts must be integral where used by repeat/array forms
- points carry units per coordinate

Acceptance:

- `(+ 5mm 2mm)` ok
- `(+ 5mm 0.5)` fails unless explicit unit cast exists
- `(* 2 5mm)` ok
- `(* 2mm 5mm)` fails unless area type is supported
- UI preserves units in param controls
- Tauri boundary stays camelCase frontend, snake_case Rust with serde rename

### Stroke, Path, Profile, And Continuous Body Ops

Problem: sketch-first modeling should support drawing a spine or contour, assigning thickness, and turning it into a printable solid without hand-authoring full BRep source.

```lisp
(stroke handle-spine
  :points ((0mm 0mm) (40mm 0mm) (52mm 18mm))
  :width 4mm
  :cap round
  :join round)

(part handle-rib
  (extrude (thicken handle-spine) 12mm))

(continuous-body handle
  :features (handle-rib side-rib mount-pad)
  :join blend
  :min-wall 2mm)
```

Rules:

- `stroke` is authoring source, not final geometry by itself.
- `thicken` turns a typed stroke/path into a closed profile where caps and joins are declared.
- `sweep` uses a profile along a path when circular/rectangular/tapered cross-sections matter.
- `extrude` remains the preferred solid operation for planar thickened profiles.
- `continuous-body` is an intent wrapper: one connected printable body with validation requirements.
- Stroke points carry length units and sketch/view frame context once spatial typing lands.
- Generated stroke features must have stable ids so later ports, keepouts, and repair actions can reference them.

Acceptance:

- open stroke plus width/caps/joins generates a closed profile deterministically
- invalid width, self-intersecting thicken, or impossible join fails before backend build
- continuous-body validation reports disconnected components, min-wall failure, self-intersection, and overhang risk
- selected stroke points or derived profiles can become named datums/ports only after explicit user/agent acceptance
- accepted stroke source can round-trip through SketchDocument replay

### Components

Problem: reusable CAD needs a real interface, not folder convention.

```lisp
(component bottle-cage
  (params ...)
  (body
    (part "BottleCage" ...))
  (exports
    (step :role primary)
    (stl :role preview)))
```

Acceptance:

- component manifest can be extracted without rendering full package payload
- params are stable and semantic
- component source/backend/artifact metadata belongs to model version/artifact, not thread
- component can be saved into package without losing `.ecky` source

### Ports

Problem: dovetail, bolt pattern, sockets, inserts, and surface patches need machine-readable contracts.

```lisp
(port dovetail_slot
  :type dovetail
  :frame (frame :origin '(0mm -47.5mm -58mm) :z-axis +z :x-axis +x)
  :clearance 0.2mm
  :length 86mm
  :profile slot_poly
  :side female
  :keepout (box 30mm 12mm 90mm))
```

Port fields:

- `type`
- `frame`
- `side`
- `profile`
- `clearance`
- `length`
- `keepout`
- `allowed-mates`
- `validation`

First built-in port types:

- `plane`
- `axis`
- `bolt_pattern`
- `dovetail`
- `snap_fit`
- `socket`
- `insert`
- `surface_patch`
- `cut_profile`

Acceptance:

- incompatible port types fail with exact raw error
- rail/slot clearance can be validated from both sides
- male/female profiles must match within declared tolerance
- no parameter key reuse for different meanings

### User-Defined Port Types

Problem: hardcoded port set is not enough for public packages.

```json
{
  "typeId": "maker.dovetail.v1",
  "requiredFields": ["frame", "profile", "clearance", "length", "side"],
  "compatibleSides": [["male", "female"]],
  "clearanceRule": "target.clearance >= source.clearance"
}
```

Rules:

- v1 supports data predicates only
- no arbitrary package solver code
- all custom types carry namespace and semver
- imported package type conflicts produce exact conflict errors

Acceptance:

- package can define port type schema
- app validates component ports against schema
- assembly can reject missing required field before render

### Mates And Assembly

Problem: assembly/fuse/mold needs source-level recipe.

```lisp
(assembly bike-bottle-holder
  (use frame-rail :from "local/frame-rail")
  (use cage :from "local/bottle-cage")
  (mate
    :source frame-rail.dovetail_rail
    :target cage.dovetail_slot
    :mode slide-in
    :clearance 0.2mm)
  (operation preserve-parts))
```

Operations:

- `place`
- `mate`
- `preserve-parts`
- `fuse`
- `cut`
- `mold`
- `blend`

Acceptance:

- separable assembly exports as multipart compound
- fused assembly exports as one solid when requested
- failed boolean reports backend raw error plus operation context
- original component recipe remains editable

## Lowerer Error Plan

Current pain: errors can happen after source already reached Steel/lowerer/backend.

Required gates:

1. Parse gate: balanced forms, valid keywords, max nesting, max node count.
2. Expand gate: macro/helper expansion depth guard.
3. Type gate: op signatures, units, point kinds, selector result kind.
4. Capability gate: source op + selected backend support.
5. Selector gate: zero/ambiguous/unsupported selector checks.
6. Lowerer gate: backend-specific unsupported surface with raw context.
7. Runtime gate: backend raw errors preserved in UI and MCP response.

Structured issue shape:

```json
{
  "code": "SELECTOR_AMBIGUOUS",
  "message": "edge selector matched 18 edges; expected 1",
  "sourceSpan": {"line": 42, "column": 7},
  "op": "chamfer",
  "selector": "(edges ledge-base ...)",
  "expected": "one edge",
  "actual": "18 edges",
  "backend": "build123d"
}
```

Acceptance:

- raw issue reaches UI and MCP unchanged
- no generic "failed to render" without raw detail
- no stack overflow for deep source
- tests cover exact error code and message body

## Phases

### Phase 1: Source Spans And Stable Node Ids

Goal: every meaningful source form can point back to source.

Work:

- parser attaches line/column spans
- compiler assigns stable node ids
- manifest stores source ids for parts/features where possible
- error type carries source id/span

BDD:

- Given invalid nested form, render fails with exact line/column.
- Given duplicate feature id, compiler fails before backend.

### Phase 2: Selector IR

Goal: selectors become typed IR, not backend helper strings.

Work:

- add `FaceSelector` and `EdgeSelector` Core IR nodes
- add selector typechecking
- add selector serialization in manifest/debug evidence
- add zero/ambiguous match issue codes

BDD:

- Given chamfer selector matches no edges, UI shows raw `SELECTOR_ZERO_MATCH`.
- Given selector matches many edges without `allowMany`, UI shows raw `SELECTOR_AMBIGUOUS`.

### Phase 3: Backend Selector Adapters

Goal: same selector semantics across build123d, FreeCAD, and internal OCCT where supported.

Work:

- build123d adapter maps selector IR to edge/face queries
- FreeCAD adapter classifies flat `shape.Edges` / `shape.Faces`
- unsupported selector fails in capability gate
- direct OCCT planner keeps internal support hidden

BDD:

- Same `.ecky` chamfer selector modifies same intended ledge on build123d and FreeCAD where both support it.
- Unsupported selector on backend reports capability error before execution.

### Phase 4: Unit Types

Goal: prevent math nonsense and make params self-describing.

Work:

- add length/angle/ratio/count param forms
- add unit parser and canonical normalization
- extend op signatures with units
- update UI param controls with unit labels

BDD:

- Adding ratio to length fails before lowering.
- Bottle diameter param shows `mm` and backend receives normalized length.

### Phase 5: Component And Port Core

Goal: reusable models expose machine-readable interface.

Work:

- add component declaration schema
- add port declaration schema
- extract component manifest
- validate port required fields
- expose ports in package header

BDD:

- Component with missing port frame fails before package export.
- Package header lists params and ports without exposing full source payload.

### Phase 6: Mate And Assembly Recipes

Goal: assemble components without manual coordinate hacks.

Work:

- add assembly recipe model
- add mate compatibility validation
- add transform solve for first mate types
  - Done, first pass: separate-parts assembly render solves exact port-frame coincidence, returns per-instance placement frames, and reports per-mate solve/clearance evidence.
  - Remaining: richer mate-specific limits/stops, profile/tolerance rules, and joined/fused op execution.
- preserve/fuse operation intent

BDD:

- Dovetail rail mates into dovetail slot with declared clearance.
- Incompatible socket/bolt pattern mate fails with exact error.

### Phase 7: Better Lowerer Guardrails

Goal: all obvious invalid source fails before backend runtime.

Work:

- max parse depth guard
- max expanded node count
- lowerer recursion guard
- capability manifest hard gate
- raw structured error plumbing through MCP/render responses

BDD:

- Very deep generated source returns `SOURCE_DEPTH_LIMIT`, no crash.
- Unsupported backend op returns `BACKEND_UNSUPPORTED_OP` with op/backend/source span.

## Test Matrix

- Parser span tests.
- Core IR selector type tests.
- Unit arithmetic type tests.
- Backend capability gate tests.
- build123d selector fixture tests.
- FreeCAD selector fixture tests where backend exists.
- MCP render failure raw error tests.
- Guide/capability drift tests.
- Package port manifest tests.
- Assembly mate compatibility tests.

## Product Payoff

- User can say "fillet inner rim only" and source can mean it.
- LLM can repair exact selector or unit error instead of guessing.
- Generated components become reusable because ports and params are real.
- Public package library can validate compatibility before render.
- Kids/noobs get clearer math: length vs angle vs ratio is visible, not hidden in numbers.

## Open Questions

1. Should selector syntax be declarative keyword lists or predicate forms?
2. Should units be part of number tokens (`12mm`) or typed param forms only in v1?
3. Should custom port schemas live in packages, global registry, or both?
4. Should selectors default to fail on many matches, or allow-many for modifiers by default?
5. How much selector provenance can build123d/FreeCAD expose consistently?
