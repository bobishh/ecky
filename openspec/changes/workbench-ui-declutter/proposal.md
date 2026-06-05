# Proposal: Workbench UI Declutter

## Intent

Strip redundant and confusing chrome from the workbench and code inspector so the
core authoring loop (prompt → render → inspect → commit) is obvious. Several
affordances either duplicate an existing one, restate state the user does not need,
or look like buttons while being something else.

## Scope

- **Code inspector diff panel removed.** The inline `.code-diff` panel (the
  "CODE DRAFT APPLIED +N/-N" block with line-by-line rows) is removed from the
  inspector. The editor shows current source only.
- **Single fork affordance.** Remove `FORK TO NEW THREAD` from the inspector
  footer. The viewport `🍴 FORK` (whole-design fork) stays as the only fork entry.
- **Commit fields labeled.** The inspector's Title and Version-name text inputs
  get visible labels and are visually separated from the action buttons, so they
  read as editable fields, not mystery buttons.
- **Direct-OCCT step status overlay removed.** The viewport
  `Direct OCCT STEP status` badge ("DIRECT OCCT STEP FAST PATH / STEP READY /
  BRep STEP artifact ready") is removed. Success needs no narration; failures are
  reported through the normal error surface.
- **One error surface.** The red `error-banner` is removed. Errors surface through
  the Ecky bubble only. Session-level errors (render, export, config, import) that
  today only reach the banner are routed into the bubble so nothing is lost.

## Out of Scope

- Changing the fork backend behavior (`forkDesign`) itself.
- Redesigning the Ecky bubble visual style.
- Changing export behavior or the export chooser.
- Removing the underlying `$session.error` state (only its banner rendering).

## Approach

UI-only change in the Svelte frontend plus the error-routing wire-up. Drive each
removal/relabel from a failing test first (assert absence, or assert the bubble
carries the error), then delete the markup, props, and dead helpers.

Affected files: `src/lib/CodePanel.svelte`, `src/lib/CodeModal.svelte`,
`src/App.svelte`, `src/lib/genie/*` (bubble error source), and the now-dead
`src/lib/directOcctStepStatus.ts` (+ test). E2E specs that assert these elements
(`app.spec.ts`, `qa.spec.ts`, `manual-code-apply.spec.ts`,
`dialogue-mcp-thread.spec.ts`) are updated to the new contract.

## Proof

- `npm run test:unit` green (incl. updated bubble error-source tests).
- `npm run typecheck` clean (no unused props/imports/helpers left behind).
- Updated e2e specs assert: no `.code-diff` panel, no `FORK TO NEW THREAD`, no
  `Direct OCCT STEP status`, no `.error-banner`, and session errors visible in the
  bubble.
