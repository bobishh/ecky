# AST-Native CAD Agent Trenches

## Purpose

Turn Ecky into agent-editable CAD system.

Not text regen.
Not raw FreeCAD macro drift.
Not direct DB writes.

Target loop:

```text
agent intent
  -> inspect AST / feature graph through MCP
  -> apply structural patch
  -> validate constraints
  -> render preview through MCP
  -> commit version through MCP
  -> user tests physical/edit behavior
```

## Operating Rules

- App state changes go through MCP only.
- No manual SQLite writes.
- No generated artifact injection outside app runtime.
- Source edits are allowed in repo files.
- Rendered model claims need MCP artifact evidence.
- UI claims need browser/e2e evidence.
- Rust changes need `cd src-tauri && cargo check`.
- Structural CAD work needs BDD outer test first where UI/user flow is involved.

## Subagent Map

| Agent | Lane | Output |
| --- | --- | --- |
| Kant | Contracts / invariants | schemas, API contracts, rejection rules |
| Helmholtz | Geometry / lowering | build123d + FreeCAD parity, topology export |
| Pasteur | Tests / verification | BDD specs, golden models, regression fixtures |
| Sagan | Agent UX / MCP workflow | tool affordances, prompt contracts, error surfaces |
| Erdos | Graphs / identity | AST identity, dependency graph, provenance graph |
| Harvey | Manufacturing / printability | FDM checks, clearances, supportless transforms |

## Current Broken Experience

Observed from film scanner work:

- Agent still patches source text, not persisted AST.
- Viewer click cannot reliably map face -> exact one input.
- Threaded/helicoid modeling exists but needs stronger op semantics and diagnostics.
- Film gate / clamp / insert details need test-by-edit loop.
- App can still mislead if semantic target has no parameter binding.
- Feature provenance is partial, not enough for general semantic transforms.

## Trench 0: Process Guardrails

Goal: stop tool misuse and make every CAD change reproducible.

### Tasks

- Add contributor doc section: model updates must use MCP `macro_preview_render` + `commit_preview_version`.
- Add debug warning if assistant/import path tries to write app DB directly in dev scripts.
- Add MCP smoke script that renders current `.ecky` source into existing thread without touching DB.
- Add checklist to PR template or docs:
  - source edited
  - MCP preview rendered
  - MCP commit produced message/model id
  - artifact digest recorded
  - no direct DB write

### Acceptance

- Given new CAD model change, agent can run one documented command/script that calls MCP.
- Then app thread gets new version.
- And no SQLite insert/update appears in workflow.

### Owner

Sagan primary. Kant reviews invariant text.

## Trench 1: Stable AST Identity Contract

Goal: one identity story.

Public stable handle: `stableNodeKey`.
`NodeId` stays debug/internal until stability proof.

### Tasks

- Define `stableNodeKey` schema:
  - source document id
  - declaration path
  - local binding name when present
  - source span digest
  - op kind
- Add identity snapshot test:
  - insert unrelated sibling
  - reorder unrelated sibling
  - change numeric literal elsewhere
  - unchanged node keeps `stableNodeKey`
- Mark `NodeId` as debug in MCP docs and responses.
- Add alias migration:
  - old span key can resolve to new stable key after whitespace-only edit.

### Acceptance

- Given `.ecky` source with params, parts, build shapes, nested calls
- When AST is inspected before and after unrelated edit
- Then unchanged source nodes keep stable keys
- And traversal `NodeId` may change without breaking public handles.

### Owner

Erdos primary. Kant reviews contract.

## Trench 2: Lossless Source/CST Editing

Goal: structural edits write source without whole-model regen.

### Tasks

- Build source/CST layer for Ecky forms:
  - params
  - parts
  - build shapes
  - result
  - call positional args
  - call keyword args
  - let/let* bindings
- Preserve comments and whitespace outside edited node.
- Add patch operations:
  - `set_number`
  - `set_string`
  - `set_select`
  - `replace_call`
  - `insert_binding`
  - `delete_binding`
  - `rename_binding_scoped`
- Add digest guards:
  - `sourceDigest`
  - `stableNodeKey`
  - `expectedNodeDigest`
- Reject macro-expanded/core-only nodes before edit.

### Acceptance

- Given source with comments and formatting
- When MCP `set_number` changes one param
- Then only numeric token diff changes
- And render preview uses patched source
- And stale digest rejects before render.

### Owner

Erdos primary. Pasteur owns tests.

## Trench 3: MCP AST Tools For Agents

Goal: agent can inspect and patch without reading full macro.

### Tools

- `ecky_ast_inspect`
  - bounded tree
  - stable keys
  - source addressability
  - editable operations
  - rejection reason
- `ecky_ast_get_node`
  - exact node by stable key
  - bounded source slice optional
- `ecky_ast_patch_validate`
  - no render
  - structured diff
  - affected nodes
  - dependency impact
- `ecky_ast_patch_preview`
  - render preview through runtime
  - returns artifact digest
- `ecky_ast_patch_commit`
  - commit existing preview
  - no raw DB access

### Acceptance

- Given user says `increase film_gap to 0.45`
- Agent calls inspect/get/validate/preview/commit
- No full macro rewrite
- Final response includes changed stable key and new MCP model id.

### Owner

Sagan primary. Erdos implements AST plumbing.

## Trench 4: Param Dependency Graph

Goal: know what a param affects before edit.

### Tasks

- Build graph:
  - param -> expression uses
  - expression -> shape binding
  - shape binding -> part
  - part -> feature/topology targets
- Expose `ecky_dependency_get`.
- Add reverse lookup:
  - face target -> feature/source nodes -> parameter keys.
- Add impact labels:
  - local
  - part-local
  - assembly-wide
  - export-affecting

### Acceptance

- Given `lens_bore_d`
- When dependency graph requested
- Then response lists carrier bore, socket bore, thread radii, stop lip bore
- And viewer click on lens bore can propose `lens_bore_d` as primary control.

### Owner

Erdos primary. Sagan validates agent usability.

## Trench 5: Feature Graph And Semantic Nodes

Goal: CAD intent exists above raw ops.

### Feature Node Types

- `film_gate`
- `film_path`
- `insert_clamp`
- `dovetail_joint`
- `helicoid_thread`
- `lens_bore`
- `stop_lip`
- `snap_hook`
- `magnet_pocket`
- `clearance_fit`

### Tasks

- Add source syntax or metadata wrapper:

```scheme
(feature lens_bore :role "lens_bore" :params (lens_bore_d)
  (difference ...))
```

- Lower feature metadata into manifest.
- Connect feature to:
  - source stable keys
  - output part ids
  - face/edge targets
  - param keys
  - ports/interfaces
- Add feature graph validator:
  - no unknown target ids
  - no stale source keys
  - no invalid parameter keys

### Acceptance

- Given film adapter
- When feature graph requested
- Then `film_path`, `insert_clamp`, `helicoid_thread`, and `lens_bore` appear
- And each feature links to source keys and rendered targets.

### Owner

Kant contracts. Helmholtz lowering. Erdos graph.

## Trench 6: Topology Provenance

Goal: face/edge selection maps back to source feature and param.

### Tasks

- During lowering, carry provenance tags per generated solid:
  - source stable key
  - feature id
  - operation kind
  - primitive id
- Export face/edge target with provenance candidates.
- Add selector resolver:
  - target id
  - durable id
  - geometry signature
  - feature role
  - parameter keys
- Add confidence:
  - exact
  - inferred
  - ambiguous
  - none

### Acceptance

- Given click on carrier inner bore face
- Then selected target maps to `lens_bore` feature
- And primary parameter list is exactly `lens_bore_d`
- Given ambiguous generated thread face
- Then UI says ambiguous and offers feature-level controls, not whole group.

### Owner

Helmholtz primary. Erdos graph. Pasteur tests.

## Trench 7: Viewer Selection UX

Goal: click/edit does not hurt orbit and does not show fake controls.

### Tasks

- Modes:
  - orbit default
  - select explicit
  - measure explicit
- In orbit:
  - no parameter popups
  - no expensive raycast/control recompute
- In select:
  - click selects face/edge/object
  - panel shows exact mapped controls
  - empty mapping shows empty state
- Cache rules:
  - project switch invalidates model/runtime assets by model id + content hash
  - preview thumbnails never poison viewport model
- Browser BDD:
  - project switch updates viewport
  - params opens without lag
  - orbit drag does not open controls
  - select click with no mapping shows empty state
  - select click with one mapped param shows one input

### Acceptance

- Given orbit mode
- When dragging model
- Then no controls open and FPS does not stall from selection work.
- Given select mode and click mapped lens bore
- Then panel shows one input: `lens_bore_d`.

### Owner

Sagan primary. Pasteur e2e.

## Trench 8: Lowerer Parity And Diagnostics

Goal: Ecky lowering strong enough to prefer over raw FreeCAD.

### Required Op Parity

- booleans: fuse/difference/intersection
- transforms: translate/rotate/scale/mirror/place
- primitives: box/cylinder/cone/sphere
- sketches: polygon/rect/rounded-rect/circle/ring
- exact ops: extrude/revolve/sweep/loft
- modifiers: fillet/chamfer/shell/offset
- arrays: linear/radial/grid/arc
- CAD ops: helical-ridge, sampled-radial-loft

### Tasks

- Golden model suite:
  - film adapter with helicoid
  - vermicomposter lid with clearance features
  - dovetail box
  - snap hook coupon
  - magnet clamp insert
- Each golden model renders via build123d and FreeCAD when supported.
- Backend failures report Ecky source span and stable key.
- Add op-level lowering diagnostics:
  - unsupported backend
  - invalid parameter
  - null boolean
  - non-manifold output
  - empty part

### Acceptance

- Given broken helical-ridge cutter
- MCP render error names source feature/key and line, not only `Null TopoDS_Shape`.
- Given golden film adapter
- build123d render emits 6 parts and step export.

### Owner

Helmholtz primary. Pasteur regression tests.

## Trench 9: Constraints And Units

Goal: catch bad CAD before render or print.

### Tasks

- Units:
  - length mm
  - angle deg/rad
  - ratio
  - count
- Constraint expressions:
  - `>=`
  - `<=`
  - clearance relation
  - wall thickness relation
  - fit relation
- Constraint examples:
  - `lens_bore_d < tunnel_aperture_h - 2`
  - `thread_clearance >= nozzle_d * 0.5`
  - `film_gap >= film_thickness + 0.1`
  - `carrier_wall >= 2 * nozzle_d`
- MCP `ecky_constraints_validate` returns:
  - pass/fail
  - involved params
  - source stable keys
  - severity

### Acceptance

- Given lens bore bigger than tunnel aperture
- validation fails before render
- error points to `lens_bore_d` and `tunnel_aperture_h`.

### Owner

Kant contracts. Harvey print constraints. Erdos dependency links.

## Trench 10: Printability Planner

Goal: supportless FDM advice with source anchors.

### Tasks

- Analyze:
  - overhang ratio
  - bridge spans
  - thin walls
  - small unsupported islands
  - thread printability
  - clearance risk
- Anchor risks to:
  - face/edge targets
  - feature ids
  - source stable keys
- Add recipes:
  - reorient
  - chamfer overhang
  - split part
  - add relief
  - increase clearance
- Preview only first. No auto-commit.

### Acceptance

- Given film adapter helicoid
- planner flags thread overhang and clearance risk
- recipes point to `helicoid_thread` feature
- no source mutation occurs until user accepts preview.

### Owner

Harvey primary. Helmholtz for geometry operations.

## Trench 11: Film Adapter Physical Edit Loop

Goal: real object iteration after foundation trenches.

### Test Coupons

- Helicoid thread coupon:
  - male/female pair
  - clearance variants `0.20`, `0.25`, `0.30`, `0.35`
  - clipped thread ends
  - lead-in chamfers
- Film path coupon:
  - lower guide + upper clamp
  - film strip pass-through
  - gap variants `0.35`, `0.45`, `0.55`
- Magnet clamp coupon:
  - 4x magnet pockets
  - polarity markers
  - removable upper mask
  - flatness check

### Adapter Slices

1. Stable 6x9 base window.
2. Insert masks for 6x4.5, 6x6, 6x9, 135.
3. Continuous film path across full adapter.
4. Magnetic upper clamp option.
5. Lens bore driven from one param.
6. Helicoid thread driven from feature params.
7. Click lens bore -> one input.
8. Click film gate -> frame/gap controls.
9. Printability planner suggests supportless orientation.

### Acceptance

- User can change `film_gap`, render through MCP, and print coupon.
- User can change `lens_bore_d`, render through MCP, and see only bore-related geometry change.
- User can click film gate and edit mask aperture without seeing unrelated helicoid params.

### Owner

Harvey physical tests. Sagan agent UX. Helmholtz geometry.

## Execution Order

### Phase 1: Stop Bleeding

- Trench 0 process guardrails.
- Trench 7 viewer mode and cache fixes.
- Trench 8 helical-ridge diagnostics.
- Film adapter v7+ fixes only via MCP.

### Phase 2: Agent Can Edit One Thing Correctly

- Trench 1 stable identity.
- Trench 2 lossless `set_number`.
- Trench 3 MCP inspect/validate/preview/commit.
- Trench 4 param dependency for direct params.

### Phase 3: Agent Understands Features

- Trench 5 feature graph source declarations.
- Trench 6 topology provenance.
- Viewer click -> exact controls.

### Phase 4: Physical CAD Loop

- Trench 9 constraints.
- Trench 10 printability planner.
- Trench 11 film adapter coupons and magnet clamp.

## Done Definition

System is not done when model renders once.

Done means:

- agent edits exact source node or feature
- MCP validates before render
- MCP preview renders artifact
- UI shows exact mapped controls
- stale/ambiguous mappings reject cleanly
- physical print feedback maps back to params/features
- direct DB writes are absent from workflow

## Open Physical Decisions

- Magnet size and thickness.
- Film stock thickness target.
- Desired clamp force.
- Lens barrel measured OD and tolerance.
- Nozzle size and layer height.
- Preferred thread clearance after coupon test.

### Status Note

- Decision placeholders are now encoded in `model-runtime/examples/physical-decision-calibration.ecky` via params and relations.
- Physical print validation is still required outside repo workflow.

## Closure Matrix

- Trench 0 — Closed in code/tests — `AGENTS.md`, `dialogue-mcp-thread.spec.ts`, `handlers.rs`
- Trench 1 — Closed in code/tests — `handlers.rs`, `contracts.rs`
- Trench 2 — Closed in code/tests — `handlers.rs`, `contracts.rs`, `runtime.rs`
- Trench 3 — Closed in code/tests — `handlers.rs`, `dialogue-mcp-thread.spec.ts`
- Trench 4 — Closed in code/tests — `handlers.rs`, `runtime.rs`
- Trench 5 — Closed in code/tests — `cad.rs`, `compiler.rs`, `mod.rs`, `runtime.rs`
- Trench 6 — Closed in code/tests — `runtime.rs`, `handlers.rs`, `context-controls.spec.ts`
- Trench 7 — Closed in code/tests — `ParamPanel.svelte`, `contextualEditing.ts`, `context-controls.spec.ts`
- Trench 8 — Closed in code/tests — `build123d_lowering_tests.rs`, `freecad_lowering.rs`, `render.rs`, `film-adapter-golden-6part.ecky`
- Trench 9 — Closed in code/tests — `physical-decision-calibration.ecky`, `handlers.rs`
- Trench 10 — Closed in code/tests — `printability.rs`, `handlers.rs`
- Trench 11 — External physical required — `helicoid-thread-coupon.ecky`, `film-path-gap-coupon.ecky`, `magnet-clamp-coupon.ecky`, `physical-decision-calibration.ecky`
