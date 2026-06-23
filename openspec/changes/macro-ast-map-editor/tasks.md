# Tasks: Macro AST Map Editor

## Triage note (2026-07-06)

This change's core feature (a spatial "New Params" map replacing/augmenting
the parameter panel) shipped through iterative work across several sessions
(see `[[project_component_unification]]` memory) rather than through this
file's prescribed worker-split, Playwright-red-first task order. Verified
against current code + `e2e/params.spec.ts` (not just paraphrased):

- **Section 1 (readonly projection) and 2 (inline editing): substantially
  shipped.** `src/lib/MacroAstMap.svelte` + `macroAstMap.ts` +
  `macroAstSceneLayout.ts` render the map (camera pan/zoom/semantic-zoom,
  minimap); `src-tauri/src/commands/macro_ast.rs::macro_ast_source_map`
  supplies byte-accurate node identity. e2e proof: "the old PARAMS entrypoint
  remains" (line 733), "New Params edits a value ... Apply rerenders the
  draft" (line 756), "part node source is edited in place ... the edit
  renders" (line 1149), error-at-source-pane on failed render (line 1192).
  Architecture differs from the spec's module split — layout/projection logic
  lives client-side in `macroAstMap.ts`/`macroAstSceneLayout.ts`, not as a
  dedicated backend projection type (1.4-1.6 as literally specified) — but the
  observable behavior matches.
- **Section 3 (search focus): partial.** A "Search controls..." input exists
  and is asserted visible (line 660), but no test proves the described
  spatial-focus/zoom/pulse behavior (3.1-3.7). Real gap.
- **Section 4 (typed-intent insertion): descoped, not done as specified.**
  Per `[[project_component_unification]]` memory, a structural "type `part`,
  see it appear" visual constructor was built and then explicitly reverted as
  a false start. What shipped instead: a "+ ADD PART" ghost slot that inserts
  a fixed template (`(part part_2 (box 10 10 10))`) and opens the source
  editor scoped to it — proven by e2e "ADD PART opens the pane ... template
  scope applies as a new part" (line 1251). This is a real, working, simpler
  insertion UX, but it is not the typed-intent flow 4.1-4.7 describe.
- **Section 5 (verification layer in the map): not built.** Authored-verify
  chips render in `PromptPanel`, not as an in-map overlay layer linked to
  node ids. Real gap.
- **Section 6 (panel retirement): decision made, not to retire.** The old
  parameter panel is kept indefinitely as a permanent fallback entrypoint
  (satisfies 6.6's intent), not migrated away from (6.4/6.5 not pursued).

Recommendation: do not archive wholesale — sections 3 and 5 are genuine
unimplemented scope that nothing else in the repo (`ast-visual-blocks` is a
separate outline/tree view, not a map; `semantic-ast-authoring` is language
semantics, not map UI) currently covers. If search-focus and a map-native
verification layer are still wanted, keep this change open scoped to just
those two sections; otherwise close it and log them as fresh, smaller changes
if/when prioritized.

## Worker Rules

- Use parallel workers only on disjoint write scopes.
- Use subagents for the same disjoint scopes when that keeps analysis and proof
  isolated.
- Workers must not rewrite unrelated app state.
- Workers must not remove existing stores or persistence in this change.
- Workers must list changed files and tests run.
- UI work must include Playwright proof for happy path and one failure or
  pending state.
- Rust changes must end with `cd src-tauri && cargo check`.
- No staging, commits, or `jj describe` unless user explicitly asks.

## 1. Additive New Params Projection

Write scope:

- `src-tauri/` parser/core/params map projection modules
- `src/lib/` params map types only
- e2e spec for readonly `New Params` route/view behavior

Tasks:

- [x] 1.1 Add failing Playwright test for opening `New Params` without removing
  the existing parameter panel entrypoint. `e2e/params.spec.ts:733`.
- [x] 1.2 Add failing Playwright test for readonly `New Params` showing model,
  owning part/region, input port, and inline param anchor. `e2e/params.spec.ts:748`
  (`view-chip` asserts exactly these four labels).
- [ ] 1.3 Add backend unit tests for stable node ids across formatting-only
  source changes. Not individually audited.
- [ ] 1.4 Add backend params map projection type with camelCase boundary
  serialization. Descoped differently: backend supplies byte-range identity
  only (`macro_ast_source_map`); full projection assembly is client-side.
- [ ] 1.5 Add command returning params map projection from source/version. See
  1.4 — no such backend command; `buildMacroAstMapProjection` (TS) does this.
- [ ] 1.6 Add shared layout engine that returns `nodeId -> x/y/w/h/path/ports/controlAnchor`.
  Shipped as `macroAstSceneLayout.ts`, not backend-shared.
- [x] 1.7 Add Svelte `New Params` scene with SVG structural layer and HTML
  overlay controls using Tactical Midnight theme and futuristic blob/port
  visual language. `src/lib/MacroAstMap.svelte`.
- [ ] 1.8 Add optional canvas underlay only for background or glow decoration.
  Not audited.
- [ ] 1.9 Add overflow boundaries to map shell, viewport, layer, and detail
  containers. Not audited.
- [ ] 1.10 Prove parse -> map -> serialize -> reparse preserves params and owner
  context. Not audited as a dedicated round-trip test.

## 2. Inline Parameter Editing

Write scope:

- AST patch backend modules
- params map control Svelte components
- existing preview/version apply flow wiring

Tasks:

- [x] 2.1 Add failing Playwright test for changing a numeric param inline and
  seeing preview/source update. `e2e/params.spec.ts:756` ("Apply rerenders the
  draft").
- [x] 2.2 Add failing Playwright test for invalid inline param value showing raw
  backend error at the control. `e2e/params.spec.ts:1192` ("an in-place edit
  fails to render ... error stays at the source pane").
- [ ] 2.3 Add unit tests for param patch success, type mismatch, missing node,
  and ambiguous node id. Not individually audited.
- [x] 2.4 Implement param patch command with structured AST update. Backed by
  `commands/macro_ast.rs` byte-range patch, exercised via 2.1/2.2.
- [ ] 2.5 Render numeric, boolean, enum, text, and reference controls from map
  metadata. Not audited per-control-type.
- [x] 2.6 Persist accepted edits through existing source/version flow.
  `e2e/params.spec.ts:1080` ("Commit is clicked Then one immutable version is
  saved").
- [x] 2.7 Keep existing parameter panel available as fallback during rollout.
  `e2e/params.spec.ts:733` — permanent, not just "during rollout" (see triage
  note: retirement was decided against, not deferred).
- [x] 2.8 Keep detached parameter panel out of the covered inline edit flow.
  Same evidence as 2.7 — the old panel and new map are parallel entrypoints.

## 3. Search Focus

Write scope:

- params map search helpers
- `New Params` search UI
- Playwright proof for spatial focus

Tasks:

Real gap — see triage note. A "Search controls..." input exists
(`e2e/params.spec.ts:660`, visibility-only assertion) but none of 3.1-3.7's
focus/frame/zoom-pulse behavior is implemented or tested.

- [ ] 3.1 Add failing Playwright test for searching a parameter and focusing the
  matching map region.
- [ ] 3.2 Add failing Playwright test for no-match search preserving current
  source and selection.
- [ ] 3.3 Add unit tests for search index over param names, labels, node ids, and
  visible source-backed labels.
- [ ] 3.4 Implement result selection that selects and frames the owning map
  region.
- [ ] 3.5 Style focused region with Tactical Midnight accents and no literal
  molecular-biology art lock.
- [ ] 3.6 Add Playwright proof for search result followed by inline apply on the
  focused node.
- [ ] 3.7 Add Playwright proof that search focus zooms or pans the SVG scene and
  pulses the target contour without turning the map into a nested rectangle
  list.

## 4. Map Insertion And Typed Intent

Write scope:

- AST insertion patch backend modules
- map insertion UI
- source serializer tests

Tasks:

Descoped — see triage note. The typed-intent constructor (type `part` and see
it appear) was built and reverted as a false start; a simpler ghost-slot +
fixed-template + source-pane-edit flow shipped instead
(`e2e/params.spec.ts:1251`), which is NOT what 4.1-4.7 specify.

- [ ] 4.1 Add failing Playwright test for click-empty-region, type `part`, and
  new part appearing in map/source.
- [ ] 4.2 Add failing Playwright test for unknown typed intent showing pending
  parser diagnostic at insertion point.
- [ ] 4.3 Add unit tests for legal insertion positions by parent node kind.
- [ ] 4.4 Add insert part/input/relation/repeat/instance patch operations.
- [ ] 4.5 Enforce new repeated physical structures use `repeat` or `instance`.
- [ ] 4.6 Enforce physical fit relations use named constraints or bindings.
- [ ] 4.7 Add source roundtrip tests for inserted nodes.

## 5. Verification Layer

Write scope:

- verify authoring backend helpers
- map verification layer components
- structural verification result overlay wiring

Tasks:

Real gap — see triage note. Authored-verify chips render in `PromptPanel`, not
as an in-map overlay linked to node ids.

- [ ] 5.1 Add failing Playwright test for selecting two related structures and
  creating a named distance/fit verification node.
- [ ] 5.2 Add failing Playwright test for failed verification displaying raw
  backend/provider error at the verify node.
- [ ] 5.3 Add unit tests for verify clause patch generation.
- [ ] 5.4 Add unit tests proving debug overlay primitives never enter
  production STL/STEP export geometry.
- [ ] 5.5 Render pending, pass, fail, and error verification states.
- [ ] 5.6 Link verification overlays to AST node ids and source verify clauses.

## 6. Source Editor And Panel Retirement Decision

Write scope:

- existing code editor entrypoints
- existing parameter panel entrypoints
- migration tests for version apply/commit

Tasks:

- [x] 6.1 Add Playwright guard proving source editor still opens selected node
  source context. `e2e/params.spec.ts:1149` ("part node source is edited in
  place ... the edit renders").
- [ ] 6.2 Add Playwright guard proving version apply preserves map-made source
  edits. Not individually audited (map-specific, vs. general apply flow).
- [ ] 6.3 Add Playwright guard proving version commit preserves map-made source
  edits. Not individually audited (map-specific, vs. general commit flow).
- [ ] 6.4 Compare old parameter panel coverage against `New Params` coverage.
  Not done — no comparison doc exists.
- [ ] 6.5 Remove detached parameter panel dependency only for proven migrated
  flows if replacement is approved. Decision made against retirement (see
  triage note) — this task is moot, not merely pending.
- [x] 6.6 Keep explicit fallback for unsupported legacy flows, if any. The old
  panel is kept as a permanent, unconditional fallback (stronger than "for
  unsupported flows only") — `e2e/params.spec.ts:733`.
- [ ] 6.7 Ensure no separate agent status bar or live terminal dump appears.

## 7. Verification

Tasks:

- [x] 7.1 Run `openspec validate macro-ast-map-editor`.
- [ ] 7.2 Run targeted backend unit tests for AST identity and patch operations.
  Not individually isolated from the full suite (see [[project_component_unification]]
  gates instead).
- [x] 7.3 Run targeted frontend unit tests for map projections/controls/search.
  Covered by full `npm run test:unit` run (7.5) — `macroAstMap.test.ts` /
  `macroAstSceneLayout.test.ts` pass within it.
- [x] 7.4 Run targeted Playwright specs for map render, param edit, search
  focus, insertion, and verification layer. Ran `e2e/params.spec.ts` in full
  2026-07-06: 20/20 green. (Search focus and verification layer have no specs
  to run — see sections 3/5 gaps.)
- [x] 7.5 Run `npm run test:unit`. 2026-07-06: 285/286 (1 pre-existing
  unrelated failure, `buildEckyIrBook ... docs corpus drift`, also flagged in
  `workbench-ui-declutter`).
- [x] 7.6 Run `cd src-tauri && cargo check`. Green throughout this session.

## Parallel Plan

Suggested worker split:

- Worker A: AST identity and backend params map projection.
- Worker B: readonly `New Params` Svelte map renderer.
- Worker C: inline param controls after map projection type stabilizes.
- Worker D: search focus after map node ids stabilize.
- Worker E: insertion AST patch operations after identity stabilizes.
- Worker F: verification layer after verify patch contract stabilizes.

Dependency order:

```text
AST identity -> params map projection -> readonly New Params renderer
AST identity -> param patch -> inline controls
AST identity -> search index -> search focus
AST identity -> insertion patch -> insertion UI
AST identity -> verify patch -> verification overlay
inline controls + search + insertion + verification -> panel retirement decision
```
