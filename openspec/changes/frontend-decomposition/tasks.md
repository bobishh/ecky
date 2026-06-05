# Tasks: Frontend Decomposition

## T0 — Compile gate (from plan Slice 0)
- [ ] Clean baseline: `cargo check` + `npm run typecheck` (drive the 32 standing TS errors to 0 — they are oversight/drift, not intentional) + `npm run test:unit` green before any cut.

## T1 — Component-test harness
- [ ] Add vitest + @testing-library/svelte + jsdom/happy-dom; wire a `test:component` script (and into CI).
- [ ] Prove the harness: one component mounted in isolation with a passing render assertion.

## T2 — Slice 1: ViewportWorkspace extraction (from plan)
- [ ] Component test for `ViewportWorkspace.svelte` (fork/export/code action presence + disabled states).
- [ ] Extract per plan; keep state/handlers in `App.svelte`.
- [ ] Move single-component presence assertions out of `qa.spec.ts` into the component test; e2e keeps cross-domain flow only.

## T3 — Following slices (viewerRuntime, agentRuntime, modelIo, WorkbenchWindows, DialogueWindowContent)
- [ ] Each slice ships component/unit tests for its seam; presence/wiring assertions migrate out of e2e per `frontend-testing` spec.

## T4 — Proof
- [ ] Component suite green in CI alongside unit + e2e.
- [ ] Net e2e pure-presence assertions decrease; `App.svelte` trends toward thin shell.

## Notes
- Slice map and target module list: `docs/app-svelte-decomposition-plan.md`.
- Recorded because button-name e2e checks and standing type errors are oversights
  under active code churn, not a chosen tradeoff — fold the cleanup into the cuts.
