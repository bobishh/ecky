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

## Roadmap Position

- Productized hidden CAD VM path: roughly 99.2%.
- Completed: truthful guides, typed Core IR checks, deterministic generative helpers, hidden direct-OCCT STEP/STL fast path, artifact-gated STEP UX, public direct-OCCT STEP failure/ready status near export actions, MCP artifact truth, full artifact manifest MCP consumer, verification artifact digests, structural STL topology/overhang metrics, dynamic list/tuple item typing, render mutation artifact digests, direct-OCCT product fixture corpus, release `.app`/DMG build proof, accepted-CAD gate, bounded accepted-CAD bounds auto-repair, bounded containment envelope expansion, BRep-derived sketch conversion with explicit provenance, multi-loop BRep-derived sketch seeds, executable topology/concavity redraw seeds, Math Lens copy for accepted-CAD repair actions, strict confirmation gating for fork actions, first-pass arbitrary 3-view candidate cell reconstruction/search that drives preview source, first accepted candidate-cell BRep path with hard STEP gate plus explicit-port component package surface, UI accept proof for candidate-cell accepted CAD, exact front-profile prism selection for concave source profiles with rectangular depth views, backend projected-loop topology validation for BRep holes, FreeCAD exact edge/face target export, and Direct OCCT exact edge/face target export into manifest/bundle topology selections.
- Remaining hard work: richer arbitrary exact BRep topology selection beyond cell unions/front-profile prisms and exported edge/face targets, richer arbitrary BRep-to-sketch topology semantics beyond loop roles/counts/edge endpoints/face centers, and demo-ready product pass.

## Current Split

- `.ecky` language: portable authoring surface.
- Core IR: backend-neutral CAD program.
- Mesh/eckyRust backend: fast preview, wall-patterns, implicit fields, mesh output, plus internal direct-OCCT STEP/STL fast path for the supported Core IR subset.
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
- Structural verifier reports parsed STL triangle count, connected component count, non-manifold edge count, and non-blocking overhang estimate metrics.
- Deterministic helpers and mesh patterns landed.
- Direct OCCT remains internal, not selectable in runtime/UI/MCP backend lists.
- Internal direct-OCCT planner seam exists under CAD host code. It accepts typed Core IR and emits ordered adapter commands for first-surface ops only.
- Internal direct-OCCT executor exports STEP/STL for literal first-surface solids including `cone`, exact `rounded-rect` and `rounded-polygon` sketch faces, `profile` holes, `make-face`, `offset` / `offset-rounded`, `bspline`, sketch-to-solid ops including `loft`, `sweep` over polyline `path` / `bezier-path`, `taper`, frame placement with `plane` / `location` / `path-frame` / `place`, `clip-box`, array ops, `twist`, transforms including `mirror`, booleans, edge modifiers, and hollowing.
- Direct OCCT exports multi-part models as a top-level OCCT compound.
- Direct OCCT native executor writes `topology.json` beside STEP/STL with per-part edge endpoint data and face center/normal/area data. Runtime maps that report into `SelectionTargetKind::Edge` / `SelectionTargetKind::Face` manifest entries and `ArtifactBundle.edgeTargets` / `ArtifactBundle.faceTargets` with matching target ids.
- EckyRust render dispatch has a hidden direct-OCCT fast path for `.ecky` Core IR when the bundled SDK is complete. It falls back to the mesh renderer on direct-OCCT blockers or unsupported surface.
- Direct runtime cleanup removes partial bundle dirs on blocked/failed native export. Service tests prove both direct STEP dispatch and fallback mesh dispatch.
- Runtime capabilities report internal direct OCCT readiness/blockers without making it an authoring backend.
- Export UI now enables STEP only when `ArtifactBundle.exportArtifacts` contains a STEP artifact; mesh/eckyRust fallback renders show direct-OCCT blocker detail instead of vague pending copy.
- MCP `artifactBundle` digest reports geometry backend, edge/face target counts, export formats, and STEP availability/path so agents can inspect artifacts before promising export.
- MCP `artifact_manifest_get` returns full validated `artifactBundle` + `modelManifest` JSON for the active target/model; `target_meta_get` includes lightweight artifact routing flags.
- MCP structural verification responses include artifact digest fields, so pass/fail checks carry export truth with them.
- MCP render mutation responses include artifact digest fields, so `params_patch_and_render`, `macro_replace_and_render`, and macro-buffer render tools carry STEP truth directly after mutations.
- Candidate-cell accept command recomputes search from the SketchDocument, selects an explicit solution, emits either cell-union source or exact front-profile prism source when rectangular depth views make that topology determinate, preserves source front-profile holes as `profile :outer/:holes`, renders through normal EckyRust dispatch, rejects mesh fallback unless a STEP export artifact exists, and runs existing STEP/FCStd hidden-line validation before returning accepted evidence.
- Accepted candidate-cell component package command requires explicit user/agent ports and known port types; heuristic geometry never becomes assembly meaning silently.
- Sketch workspace can create a reusable accepted-BRep package from an accepted STEP candidate with explicit `front_mount` / `mechanical.plane.mount.v1` port evidence, optional explicit control surface (`params`, `uiSpec`, `initialParams`), and raw backend package errors surfaced unchanged.
- BRep hidden-line projections can carry projected loops with `outer` / `hole` / `unknown` roles. When loops are absent, validation derives closed loops from edge chains, classifies roles by containment parity, and rejects loop-count or hole-count mismatches against source sketch profiles.
- FreeCAD runner reports exact BRep edge endpoint targets from `Shape.Edges` and face center/normal/area targets from `Shape.Faces`. Rust converts those into `SelectionTargetKind::Edge` / `SelectionTargetKind::Face` manifest entries plus `ArtifactBundle.edgeTargets` / `ArtifactBundle.faceTargets` entries with matching target ids, so viewer/assembly code can select real model topology instead of guessed semantic parts.
- Cached FreeCAD bundles reconcile edge-target labels/editability against the current manifest while preserving endpoint coordinates, so accepted bindings and later port labels do not leave stale edge overlays.
- `.ecky` now exposes exact-only `sampled-radial-loft` for formula-driven sampled radial section lofts. FreeCAD/build123d lower it natively; build123d and internal direct-OCCT sampled shell planning both synthesize inner sampled radial lofts for `(shell wall (sampled-radial-loft ...))`; `EckyRust` render dispatch now probes direct-OCCT first for sampled radial lofts and falls forward to build123d when the internal SDK is unavailable or blocked, while mixed `wall-pattern` + `sampled-radial-loft` sources still reject early instead of pretending one backend can do both.
- Component ports can carry `targetIds`, and accepted-BRep package creation validates those ids against accepted artifact bundle edge/face targets before preserving them in the package.
- Generic runtime/exact bundles can now be wrapped into portable component package projects without sketch-accept coupling. Packaging prefers reusable source when `macroPath` exists, falls back to STEP when needed, preserves source language/backend hints for source-backed components, auto-derives full source param surface (`params`, `uiSpec`, `initialParams`) from reusable source when explicit package params are omitted, preserves explicit package-request `uiSpec` and `initialParams` for non-source-backed bundles, allows zero-port decorative/source-backed components, validates explicit port `targetIds` against runtime manifest plus bundle topology before install/reuse, backfills empty param surfaces for installed legacy source-backed packages on resolve, merges stored `initialParams` with runtime overrides during installed component and assembly render, exposes cheap installed-component and installed-assembly controls resolve without rendering, and echoes merged values back from installed-component runtime responses.
- Installed component assemblies can resolve package-local `.ecky` / `.step` / `.FCStd` sources back into runtime bundles, solve first-pass exact port-frame placement for separate-parts mates, return per-instance `placementFrame` data, emit per-mate solve/clearance evidence plus per-operation apply evidence/warnings and `fusionZoneIdsByInstance`, block joined/fused runtime execution when target instances lack op-capable fusion zones, synthesize joined-assembly runtime bundles when `output.mode = JoinedAssembly` and operations are absent, pure `Fuse` groups, or narrow ordered `Cut` groups (`targetInstanceIds[0]` base, later ids cutters), synthesize narrow fused-solid runtime bundles when `output.mode = FusedSolid` and pure `Fuse` groups cover the whole assembly, and export solved assemblies as placed 3MF build items or multipart STL zips with baked placement transforms while broader boolean assembly ops remain parked.
- Direct OCCT blocker summaries include runtime root, checked include candidates, and the full emitted-source header set, so stale packaged runtimes or wrong `BUILD123D_RUNTIME_DIR` no longer collapse to a vague missing-header message or a later native compile failure.
- Core IR typechecker now propagates known `map` source item kinds into map bodies, preserves let-bound point-list item kinds, and rejects raw 2-component lists where 3D points are required.
- Core IR typechecker now infers dynamic `map` output item kinds, recognizes numeric 2/3-tuples as point2/point3 items, rejects known non-point lists passed to point-list CAD ops, and validates `apply` variadic shape list items.
- Expanded compiler `enumerate` now matches runtime order `(index value)` across direct calls and static destructuring.
- Tauri release build now pre-cleans stale writable DMG temp mounts/images before bundling; release `.app` and DMG build verified locally.

## Next CAD VM Tranche Checklist

- Track `.ecky` language improvements in [Ecky Language Improvement Roadmap](./ecky-language-improvement-roadmap.md): face/edge selectors, units/types, component/port contracts, and earlier lowerer errors.
- More Typed Core IR tightening: richer heterogeneous tuple schemas for `zip` / `enumerate` / destructuring.
- Promote candidate-cell reconstruction/search beyond cell-union, exact front-profile-prism STEP proof, exported FreeCAD edge/face targets, and Direct OCCT edge/face targets into richer exact BRep topology selection.
- Demo-ready product pass.

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
- `stroke`: `List<Point2> Length Cap Join -> Stroke`
- `thicken`: `Stroke -> Sketch`
- `continuous-body`: `Solid... ValidationPolicy -> Solid`

Checker behavior:

- `Any` stays permissive where compiler cannot prove type.
- Lists stay permissive until element-kind tracking exists.
- Errors name op, argument, expected kind, actual kind, and source span when present.
- Compiler fails before backend lowering on obvious type bugs.

## Phase 2: Structural Proof Harness

Every render should return deterministic facts:

- STL exists and is non-empty.
- STL has triangles.
- STL triangle count.
- STL non-manifold edge count.
- STL connected component count.
- STL overhang triangle count/ratio estimate.
- manifest has parts.
- part bounds are finite and non-degenerate.
- volume and area are finite and positive when available.
- viewer assets reference known parts.
- multipart AABB distance catches obvious disconnected pieces.

Later checks:

- minimum wall estimate.
- self-intersection where backend exposes it.
- stroke-thicken self-intersection and join validity.
- continuous-body checks for single connected printable mass.

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
- Internal executor exists for literal first-surface solids, sketch faces, sketch-to-solid ops, frame placement, clip boxes, arrays, transforms, booleans, edge modifiers, hollowing, and exports: `box`, `sphere`, `cylinder`, `cone`, `circle`, `rectangle`, `rounded-rect`, `rounded-polygon`, `polygon`, `profile`, `make-face`, `offset`, `offset-rounded`, `path`, `bezier-path`, `bspline`, `plane`, `location`, `path-frame`, `place`, `clip-box`, `linear-array`, `radial-array`, `grid-array`, `arc-array`, `extrude`, `revolve`, `loft`, `sweep`, `taper`, `twist`, `translate`, `rotate`, `scale`, `mirror`, `compound`, `union`, `difference`, `intersection`, `fillet`, `chamfer`, `shell`, STEP, STL.
- Multi-part Core IR programs export as a top-level compound, not a single-part-only executor special case.
- Internal runtime bundle adapter writes manifest, preview STL, primary STEP artifact, and face topology targets for direct OCCT exports while presenting public geometry backend as `mesh`; multi-part exports preserve part bindings in the manifest.
- EckyRust render dispatch has a hidden direct-OCCT fast path for `.ecky` Core IR when the bundled SDK is complete. It falls back to the mesh renderer on direct-OCCT blockers or unsupported surface.
- Planner supports ordered commands for first-surface primitives, sketches, polyline paths, frames, clipping, arrays, booleans, transforms including mirror, shell, fillet, chamfer, taper, and rounded offset.
- Planner rejects typed holes and unsupported ops before any future OCCT runtime adapter can execute. Executor resolves Core parameter defaults plus runtime overrides before C++ generation.
- Bundled build123d runtime includes OCP OCCT 7.8.1 dylibs, including STEP/STL libraries.
- `npm run build123d:prepare` prepares the build123d runtime, then runs `scripts/prepare_occt_headers.sh`.
- `scripts/prepare_occt_headers.sh` fetches OCCT 7.8.1 source headers into `.dist/build123d-runtime/include/opencascade`.
- If UI reports `Direct OCCT unavailable: OCCT include directory missing`, the local runtime was prepared without headers or rebuilt by an old command. Run `npm run build123d:prepare`; order matters because `prepare_build123d_runtime.sh` recreates `.dist/build123d-runtime`.
- Native probes and executor fixtures compile against bundled headers/OCP dylibs and export real STEP/STL when the SDK is complete. Production executor fixtures also write face topology reports used by runtime manifests/bundles.
- OCP dylibs use `/DLC/OCP/.dylibs/...` install names. Shared native export runner rewrites executable load commands to actual bundled dylib paths for both probe and production executor binaries.
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
