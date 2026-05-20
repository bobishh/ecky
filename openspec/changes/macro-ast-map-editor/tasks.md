# Tasks: Macro AST Map Editor

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

- [ ] 1.1 Add failing Playwright test for opening `New Params` without removing
  the existing parameter panel entrypoint.
- [ ] 1.2 Add failing Playwright test for readonly `New Params` showing model,
  owning part/region, input port, and inline param anchor.
- [ ] 1.3 Add backend unit tests for stable node ids across formatting-only
  source changes.
- [ ] 1.4 Add backend params map projection type with camelCase boundary
  serialization.
- [ ] 1.5 Add command returning params map projection from source/version.
- [ ] 1.6 Add shared layout engine that returns `nodeId -> x/y/w/h/path/ports/controlAnchor`.
- [ ] 1.7 Add Svelte `New Params` scene with SVG structural layer and HTML
  overlay controls using Tactical Midnight theme and futuristic blob/port
  visual language.
- [ ] 1.8 Add optional canvas underlay only for background or glow decoration.
- [ ] 1.9 Add overflow boundaries to map shell, viewport, layer, and detail
  containers.
- [ ] 1.10 Prove parse -> map -> serialize -> reparse preserves params and owner
  context.

## 2. Inline Parameter Editing

Write scope:

- AST patch backend modules
- params map control Svelte components
- existing preview/version apply flow wiring

Tasks:

- [ ] 2.1 Add failing Playwright test for changing a numeric param inline and
  seeing preview/source update.
- [ ] 2.2 Add failing Playwright test for invalid inline param value showing raw
  backend error at the control.
- [ ] 2.3 Add unit tests for param patch success, type mismatch, missing node,
  and ambiguous node id.
- [ ] 2.4 Implement param patch command with structured AST update.
- [ ] 2.5 Render numeric, boolean, enum, text, and reference controls from map
  metadata.
- [ ] 2.6 Persist accepted edits through existing source/version flow.
- [ ] 2.7 Keep existing parameter panel available as fallback during rollout.
- [ ] 2.8 Keep detached parameter panel out of the covered inline edit flow.

## 3. Search Focus

Write scope:

- params map search helpers
- `New Params` search UI
- Playwright proof for spatial focus

Tasks:

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

- [ ] 6.1 Add Playwright guard proving source editor still opens selected node
  source context.
- [ ] 6.2 Add Playwright guard proving version apply preserves map-made source
  edits.
- [ ] 6.3 Add Playwright guard proving version commit preserves map-made source
  edits.
- [ ] 6.4 Compare old parameter panel coverage against `New Params` coverage.
- [ ] 6.5 Remove detached parameter panel dependency only for proven migrated
  flows if replacement is approved.
- [ ] 6.6 Keep explicit fallback for unsupported legacy flows, if any.
- [ ] 6.7 Ensure no separate agent status bar or live terminal dump appears.

## 7. Verification

Tasks:

- [x] 7.1 Run `openspec validate macro-ast-map-editor`.
- [ ] 7.2 Run targeted backend unit tests for AST identity and patch operations.
- [ ] 7.3 Run targeted frontend unit tests for map projections/controls/search.
- [ ] 7.4 Run targeted Playwright specs for map render, param edit, search
  focus, insertion, and verification layer.
- [ ] 7.5 Run `npm run test:unit`.
- [ ] 7.6 Run `cd src-tauri && cargo check`.

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
