# Tasks: Workbench UI Declutter

## T1 — Remove inspector diff panel
- [x] Update e2e/unit to assert `.code-diff` / `[data-testid="code-diff-panel"]` absent.
- [x] Remove diff template + CSS + `diffCode` derived from `CodePanel.svelte`.
- [x] Remove `diff*` props from `CodePanel.svelte` and `CodeModal.svelte`.
- [x] Remove `sessionCodeDiffView` plumbing + diff props in `App.svelte`; drop unused imports.

## T2 — Single fork affordance
- [x] Update `app.spec.ts` / `qa.spec.ts` to drop `FORK TO NEW THREAD` assertions.
- [x] Remove `FORK TO NEW THREAD` button, `onFork`, `handleFork`, `'forking'` state from `CodeModal.svelte`.
- [x] Remove `onFork` wiring in `App.svelte`; drop `forkManualVersion` import (now unused).

## T3 — Label commit fields
- [x] Add visible labels for Title and Version-name inputs; separate them from the action buttons (border divider).

## T4 — Remove Direct-OCCT step status overlay
- [x] Remove the three `Direct OCCT STEP status` overlay tests in `qa.spec.ts` (export-chooser tests keep covering availability).
- [x] Remove the overlay block + CSS + `directOcctStepStatus` derived + import in `App.svelte`.
- [x] Delete `src/lib/directOcctStepStatus.ts` and its test.

## T5 — One error surface (bubble)
- [x] Add bubble error-source unit tests: session error appears + takes priority in bubble presentation.
- [x] Route `$session.error` into the bubble via `resolveGenieBubblePresentation` (`sessionError` source, top priority).
- [x] Remove `error-banner` markup + CSS + `dismissError`/`copyError`/`errorCopied`/`errorCopyResetTimer` (dead).
- [x] Update `manual-code-apply.spec.ts` / `dialogue-mcp-thread.spec.ts` to assert errors via bubble, not banner.

## T6 — Proof
- [x] `npm run test:unit` green for touched areas (1 unrelated pre-existing failure: `eckyIrGuide` docs corpus drift).
- [x] `npm run typecheck` clean for touched files (32 errors are all pre-existing, unchanged count).
- [ ] Run updated e2e specs — updated to new contract but not executed this session (heavy Tauri/runtime spin-up); run before merge.
