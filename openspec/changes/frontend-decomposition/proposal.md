# Proposal: Frontend Decomposition

## Intent

Break the workbench god-components (`App.svelte` ~4.1k, `ParamPanel.svelte`
~3.8k, `Viewer.svelte` ~2.6k) into thin shells + extracted components and
composables, one working seam at a time, with no behavior change. The slice map
lives in `docs/app-svelte-decomposition-plan.md`; this change is its OpenSpec
home and adds the testing strategy that the slice work must follow.

This is the frontend counterpart to `architecture-decomposition` (which paid
down the Rust god-files). Same discipline: mechanical splitting, every existing
test green, no Core IR or contract shape change.

## Findings (grounded in code)

- `src/App.svelte`: workbench orchestrator; owns viewport cache, project switch,
  params, dialogue, agent state, export/import, sketch preview, window layout —
  small fixes carry large blast radius.
- Target shape and per-slice extraction map are already specified in
  `docs/app-svelte-decomposition-plan.md` (ViewportWorkspace, WorkbenchWindows,
  DialogueWindowContent components; viewerRuntime, agentRuntime, modelIo
  composables).
- **Test pyramid is missing its middle.** Only two layers exist today: pure
  function unit tests (`tsx --test` on `*.test.ts`) and full-app Playwright e2e.
  There is no component-test harness (no vitest / @testing-library/svelte). So
  cheap assertions — "FORK button is present", "error text shows", "field has a
  label" — are forced up into slow, brittle full-Tauri e2e. The recent
  workbench-ui-declutter change had to edit four e2e specs just to assert button
  presence; that is the smell.

## Scope

- Establish a Svelte component-test harness (vitest + @testing-library/svelte +
  jsdom/happy-dom) wired into `npm run test:unit` (or a sibling `test:component`).
- For each extracted seam in the plan, add component tests that cover the
  component's own rendering/wiring contract, and pull the corresponding
  presence/label/disabled-state assertions OUT of the full-app e2e specs.
- Execute the slices in the plan's order (Slice 0 compile gate → ViewportWorkspace
  → viewerRuntime → …), each behind its BDD outer loop.

## Out of Scope

- Any Core IR / `contracts.ts` shape change.
- New features. Pure decomposition + test-layer introduction.
- Rewriting Three.js viewer internals (`Viewer.svelte` extraction is a later
  slice, not this change's first target).

## Success Criteria

- A component-test harness exists and runs in CI alongside unit + e2e.
- `App.svelte` shrinks toward a thin root shell per the plan; first landed slice
  (ViewportWorkspace) has component tests, not just e2e.
- Net e2e assertion count for pure UI presence drops as those assertions move to
  component tests; e2e is reserved for true cross-domain flows.
- `npm run test:unit` and the new component suite stay green; existing e2e stays
  green for the flows it still owns.
