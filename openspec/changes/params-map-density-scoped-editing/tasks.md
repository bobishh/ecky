# Tasks: Params Map Density And Scoped Editing

## Worker Rules

- The two slices BOTH write `src/lib/MacroAstMap.svelte` and
  `e2e/params.spec.ts` — run them sequentially, never in parallel.
- Slice 1 (scoped editing) goes first: it fixes a live offset-drift bug.
- BDD red first: every behavior lands as a failing test before implementation.
- No staging, commits, or `jj describe`.
- Workers must list changed files and tests run.

## 1. Scoped Source Editing

Write scope: `src/lib/MacroSourcePane.svelte`, `src/lib/MacroAstMap.svelte`,
`src/lib/macroAstMap.ts` (splice helper), `e2e/params.spec.ts`, new unit test
file for the splice helper.

- [x] 1.1 Add failing unit tests for `spliceMacroSource(base, start, end, slice)`
  (middle / start / end / empty slice / degenerate range), then implement it
  as a pure exported helper.
- [x] 1.2 Update the existing in-place part edit Playwright spec
  (`e2e/params.spec.ts` ~1149) red-first: pane must contain the selected
  part's source and must NOT contain another part's source; apply still
  rerenders.
- [x] 1.3 Add failing Playwright spec for the dirty-switch guard: edit the
  pane draft, dblclick another node, pane keeps the draft and shows the
  inline unsaved-draft message; with a clean pane the switch swaps slice and
  label.
- [x] 1.4 Update the ADD PART Playwright spec (~1251) red-first: pane shows
  only the inserted template; applying splices it in and the new part renders.
- [x] 1.5 Rework `MacroSourcePaneState` to `baseCode` + slice per design D2;
  editor doc = slice; APPLY = splice + `onApplyMacroCode`; drop the scope
  decoration machinery from `MacroSourcePane` (D4).
- [x] 1.6 Implement the dirty-switch guard in `MacroAstMap` (D3), reusing the
  pane error slot for the inline message.
- [x] 1.7 Keep the failed-apply error surface at the pane (existing spec
  ~1192 stays green).

## 2. Part Density Collapse

Write scope: `src/lib/macroAstSceneLayout.ts`, `src/lib/MacroAstMap.svelte`,
`src/lib/macroAstSceneLayout.test.ts`, `e2e/params.spec.ts`.

- [x] 2.1 Add failing unit tests: part with >6 params and no
  `expandedPartIds` entry → constant collapsed height, no param nodes, no
  param connectors; same part in `expandedPartIds` → today's grid; part with
  ≤6 params → unchanged; threshold boundary 6 vs 7.
  Evidence: 5 new tests added to `src/lib/macroAstSceneLayout.test.ts`,
  confirmed red (missing `PART_COLLAPSE_THRESHOLD` export) before
  implementation; all 5 green after 2.3.
- [x] 2.2 Add failing Playwright spec: dense part renders collapsed with a
  param-count chip and no inline controls; expanding shows controls; editing
  a param works; collapsing restores the compact node.
  Evidence: new test "Given a dense part When New Params opens Then it
  renders collapsed with a count chip until expanded" in `e2e/params.spec.ts`
  (new `dense-macro` mock scenario, 8 params on one part), confirmed red
  (chip locator not found) before implementation; green after 2.4.
- [x] 2.3 Implement `hints.expandedPartIds` + collapsed layout in
  `buildMacroAstSceneLayout` (D5).
  Evidence: `PART_COLLAPSE_THRESHOLD = 6` exported; `MacroAstSceneNodeLayout`
  gained `paramCount`/`collapsed`; collapsed height fixed at
  `partHeaderH + collapsedChipRowH + partPadBottom` (~64px) regardless of
  param count; params/param-nodes omitted when collapsed; root→part
  connector unaffected.
- [x] 2.4 Implement collapse/expand UI in `MacroAstMap.svelte`: count chip,
  toggle affordance, `expandedParts` session state; dblclick still opens
  source.
  Evidence: `expandedParts = $state(new Set<string>())`, passed as
  `expandedPartIds` into the layout call; `.macro-ast-part-collapse-chip`
  button (`data-testid="macro-ast-part-collapse-chip"`) rendered on part
  nodes with `paramCount > PART_COLLAPSE_THRESHOLD`, `stopPropagation` on
  click so dblclick-to-edit-source is untouched; toggling assigns a new Set.
- [x] 2.5 Auto-expand owning part in focus flows (`focusSceneField`,
  `selectSceneFieldValue`, `highlightedParamKey` path) before DOM focus.
  Evidence: new `findOwningPartId(root, fieldKey)` pure helper in
  `macroAstMap.ts` (unit-tested in `macroAstMap.test.ts`); `focusSceneField`
  / `selectSceneFieldValue` now route through `expandOwningPartThen`, which
  expands the collapsed owning part and defers to a double
  `requestAnimationFrame` (layout re-run + DOM) before continuing; a new
  `$effect` on `highlightedParamKey` expands the owning part so highlighted
  fields in a collapsed part become visible. E2E coverage for this path was
  not added: `highlightParam` is only reachable through a real Tauri
  `mcp://ui-dispatch` event and `listen()` is not mocked in
  `e2e/params.spec.ts`, so firing it cheaply from a test isn't possible
  without new test-only plumbing; the decision function and manual wiring
  are covered per the task's stated fallback.

## 3. Verification

- [x] 3.1 `openspec validate params-map-density-scoped-editing` — valid.
- [x] 3.2 `npm run test:unit` 285/286 (1 pre-existing docs-corpus-drift failure). NOTE: the script glob misses `src/lib/*.test.ts` top-level files; `npx tsx --test src/lib/macroAstSceneLayout.test.ts src/lib/macroAstMap.test.ts` run directly — all pass.
- [x] 3.3 `npx playwright test e2e/params.spec.ts` — 22/22.
- [x] 3.4 Skipped: new specs use self-contained mock scenarios inside `params.spec.ts`; no shared fixtures touched. (`version-timeline-verify.spec.ts` has one pre-existing red, reproduced with this change stashed.)
