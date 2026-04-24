# EckyRust CAD VM Roadmap

## Meaning

EckyRust CAD VM means controlled CAD runtime, not JVM-scale virtual machine.

Pipeline:

```text
.ecky source
  -> parse
  -> expand helpers
  -> typecheck Core IR
  -> lower to backend
  -> render/export
  -> structural verification
  -> repair feedback
```

Goal: make model output boringly checkable before any backend executes it.

## Current Split

- `.ecky` language: portable authoring surface.
- Core IR: backend-neutral CAD program.
- Mesh/eckyRust backend: fast preview, wall-patterns, implicit fields, mesh output only.
- build123d backend: Python OCCT-backed exact CAD.
- FreeCAD backend: FreeCAD/OCCT exact CAD.
- Future direct OCCT backend: Rust-owned exact CAD adapter, not user-selectable today.

Do not let guide text imply a backend supports an op unless capability manifest and tests prove it.

## Current Landed Truth

- Canonical language surface manifest exists.
- Generation and MCP guides read canonical manifest data.
- Typed holes compile and typecheck as planning placeholders.
- Unfilled typed holes reject during lowering before backend execution.
- Structural verifier separates unreadable STL from readable zero-triangle STL.
- Deterministic helpers and mesh patterns landed.
- Direct OCCT remains internal, not selectable in runtime/UI/MCP backend lists.
- Internal direct-OCCT planner seam exists under CAD host code. It accepts typed Core IR and emits ordered adapter commands for first-surface ops only.
- Internal direct-OCCT executor exports STEP/STL for literal box/sphere/cylinder, circle/rectangle/polygon extrusion, translate, compound, union, difference, and intersection.
- Runtime capabilities report internal direct OCCT readiness/blockers without making it an authoring backend.

## Next CAD VM Tranche Checklist

- JSON manifest consumers in agent tools.
- Direct OCCT runtime adapter connection to render dispatch after edge/shell coverage and UX gating are ready.
- Render fixture expansion.
- Typed Core IR tightening.

## Phase 1: Typed IR And Signatures

Add operation signatures over Core IR.

Examples:

- `box`: `Number Number Number -> Solid`
- `circle`: `Number -> Sketch`
- `extrude`: `Sketch Number -> Solid`
- `translate`: `Number Number Number Geometry -> Geometry`
- `union`: `Solid... -> Solid`
- `polygon`: `List<Point2> -> Sketch`
- `path`: `List<Point3> -> Path`

Checker behavior:

- `Any` stays permissive where compiler cannot prove type.
- Lists stay permissive until element-kind tracking exists.
- Errors name op, argument, expected kind, actual kind, and source span when present.
- Compiler fails before backend lowering on obvious type bugs.

## Phase 2: Structural Proof Harness

Every render should return deterministic facts:

- STL exists and is non-empty.
- STL has triangles.
- manifest has parts.
- part bounds are finite and non-degenerate.
- volume and area are finite and positive when available.
- viewer assets reference known parts.
- multipart AABB distance catches obvious disconnected pieces.

Later checks:

- watertight/manifold.
- connected components count.
- overhang estimate.
- minimum wall estimate.
- self-intersection where backend exposes it.

## Phase 3: Direct OCCT Adapter Spike

Keep FreeCAD/build123d. Add direct OCCT as user-selectable backend only after Core IR typechecking and adapter execution are useful.

First direct OCCT surface:

- primitives: `box`, `cylinder`, `sphere`
- sketches: `circle`, `rectangle`, `polygon`
- surfaces: `extrude`, `revolve`
- booleans: `union`, `difference`, `intersection`
- edges: `fillet`, `chamfer`
- hollowing: `shell`
- exports: STEP, STL

Adapter rule: backend code receives typed Core IR, never raw LLM strings.

Current status:

- Planner exists.
- Internal executor exists for literal first-surface solids and basic sketch extrusion: `box`, `sphere`, `cylinder`, `circle`, `rectangle`, `polygon`, `extrude`, `translate`, `compound`, `union`, `difference`, `intersection`.
- Internal runtime bundle adapter writes manifest, preview STL, and primary STEP artifact for direct OCCT exports while presenting public geometry backend as `mesh`.
- Planner supports ordered commands for first-surface primitives, sketches, booleans, transforms, shell, fillet, and chamfer.
- Planner rejects typed holes and unsupported ops before any future OCCT runtime adapter can execute.
- Bundled build123d runtime includes OCP OCCT 7.8.1 dylibs, including STEP/STL libraries.
- `scripts/prepare_occt_headers.sh` fetches OCCT 7.8.1 source headers into `.dist/build123d-runtime/include/opencascade`.
- Native probes compile against bundled headers/OCP dylibs and export real STEP/STL when the SDK is complete.
- OCP dylibs use `/DLC/OCP/.dylibs/...` install names. Native probe rewrites executable load commands to actual bundled dylib paths; production executor packaging needs same strategy.
- Do not list direct OCCT in source/backend settings, MCP schemas, or language manifests until render dispatch coverage and UX are ready.

## Phase 4: Typed Holes

Typed holes are design placeholders, not runtime geometry.

Typed holes now compile and typecheck as planning placeholders. They are not runtime geometry. Unfilled holes must fail during render/lowering before any backend executes. Do not use `(hole ...)` for finished renderable models.

Example future shape:

```lisp
(part clamp
  (difference
    (hole :type solid :goal "snap clip outer body")
    (hole :type solid :goal "tube clearance volume")))
```

Compiler should report:

- hole id
- required type
- surrounding op
- design goal
- constraints available from params

Agent fills holes iteratively.

## Phase 5: Search And Ranking

CAD invention should be search, not one-shot generation.

Loop:

```text
draft -> compile -> render -> measure -> mutate -> rank -> explain
```

Rank signals:

- constraints passed
- structural verification passed
- printable wall/overhang estimates
- volume/material use
- symmetry/editability
- novelty/aesthetic score
- user prompt fit

## Guide Rules

- Guides come from capability data, not hand-written stale lists.
- Backend guide hides unsupported backend ops.
- Mesh-only `wall-pattern` stays out of FreeCAD/build123d guides.
- `map`/`range` are expression/list forms, not model-clause generators.
- Structural verification is first authority; screenshot/VLM is secondary.

## Near-Term Test Matrix

- Core IR typechecker accepts valid primitives/surfaces/transforms.
- Core IR typechecker rejects Solid passed where Sketch required.
- Core IR typechecker rejects Number passed where geometry required.
- Structural verifier rejects zero-triangle STL.
- Guide tests assert no mesh-only ops in exact backend guides.
- Render fixtures cover organic loops, chaotic point helpers, and mesh implicit wall modes.
