# Tasks: Session Collaboration Visibility

## Worker Rules

- Use parallel workers only on disjoint write scopes.
- Workers must not rewrite unrelated app state.
- Workers must not remove existing stores or persistence in this change.
- Workers must list changed files and tests run.
- Main thread integrates and resolves event naming.
- UI work must include Playwright proof for happy path and one failure/pending
  state.

## 1. T1 - Session Event Model and Composer

Write scope:

- new `src/lib/sessionActivity.ts`
- new `src/lib/sessionActivity.test.ts`
- optional `src/lib/types/sessionActivity.ts`

Tasks:

- [x] 1.1 Define `SessionEvent`, actor, artifact, diff, and severity types.
- [x] 1.2 Add pure `appendSessionEvent(events, event)` helper with stable sort.
- [x] 1.3 Add `composeSessionActivity(...)` projection for active thread/version.
- [x] 1.4 Add `composeBubbleEvent(...)` selection rules.
- [x] 1.5 Add `composeCodeDiffView(...)` for latest macro diff.
- [x] 1.6 Add unit tests for event ordering, filtering, bubble selection, and
  macro diff selection.

## 2. T2 - Bubble Click and Activity Window

Write scope:

- `src/lib/VertexGenie.svelte`
- `src/App.svelte` activity-window wiring only
- optional new `src/lib/SessionActivityWindow.svelte`

Tasks:

- [x] 2.1 Add bubble primary click callback without breaking copy/dismiss.
- [x] 2.2 Add accessible title/label for opening session activity.
- [x] 2.3 Add activity window shell with event list and detail pane.
- [x] 2.4 Show full event text without truncation in activity detail.
- [x] 2.5 Preserve Tactical Midnight theme and overflow boundaries.
- [x] 2.6 Add Playwright proof: long bubble opens full activity text.

## 3. T3 - Code Editor Macro Diff Panel

Write scope:

- `src/lib/CodePanel.svelte`
- new `src/lib/codeDiff.ts`
- new `src/lib/codeDiff.test.ts`

Tasks:

- [x] 3.1 Add line-based macro diff helper.
- [x] 3.2 Add `LAST MACRO DIFF` panel below or beside CodeMirror editor.
  (Was checked earlier but never wired — `codeDiff.ts` had zero consumers.
  Now real: `MacroDiffPanel.svelte` rendered inside `CodeModal` under the
  editor, fed by `composeCodeDiffView` from App.)
- [x] 3.3 Show actor, timestamp, old/new summary, and changed lines.
  (`composeMacroDiffPanelModel` in `src/lib/macroDiffPanel.ts`, unit tests in
  `macroDiffPanel.test.ts`.)
- [x] 3.4 Keep current CodeMirror editor editable.
- [x] 3.5 Evaluate CodeMirror merge/diff extension; use only if low-risk.
  DECISION: rejected. Custom `codeDiff.ts` LCS diff already ships tested and
  the panel is a read-only projection; `@codemirror/merge` adds a dependency
  and an editable two-pane merge surface we don't need, plus theme-clash risk
  with the custom ecky highlight modes.
- [x] 3.6 Add unit tests for insert/delete/change diff cases.
- [x] 3.7 Add Playwright proof: macro patch event opens code editor with diff.
  (`manual-code-apply.spec.ts` — "LAST MACRO DIFF with actor and changed
  lines".)

## 4. T4 - Preview and Validation Extended Detail

Write scope:

- new `src/lib/SessionPreviewDetail.svelte`
- `src/App.svelte` detail wiring only
- optional helpers in `src/lib/sessionActivity.ts`

Tasks:

- [x] 4.1 Render preview event image/artifact detail in activity window.
- [x] 4.2 Render validation summary and raw issue list.
- [x] 4.3 Link related render/validation/preview events.
  (`relatedSessionEvents` in `sessionActivity.ts` — render-cycle events
  sharing the anchor's version; clickable RELATED section in
  `SessionActivityWindow.svelte`; unit tests in `sessionActivity.test.ts`.)
- [x] 4.4 Show raw backend/provider error bodies for failed events.
- [x] 4.5 Add Playwright proof: preview event opens extended preview detail.
  (`genie.spec.ts` — "extended preview detail shows the artifact section".)
- [x] 4.6 Add Playwright proof: validation failure shows raw issue details.
  (`genie.spec.ts` — "agent error state ... detail shows raw issue body".)

## 5. T5 - Event Emission at Existing Command Boundaries

Write scope:

- `src/App.svelte`
- `src/lib/controllers/manualController.ts`
- `src/lib/stores/history.ts`
- no UI component edits except event dispatch props

Tasks:

- [x] 5.1 Emit `macro_patch_applied` on manual code apply.
- [x] 5.2 Emit `params_changed` on param apply/commit with old/new values.
- [x] 5.3 Emit `render_started`, `render_succeeded`, and `render_failed`.
- [x] 5.4 Emit `validation_reported` from agent draft feedback and verifier
  results.
- [x] 5.5 Emit `preview_updated` from version/draft preview updates.
- [x] 5.6 Emit `version_committed` from manual/imported commit paths.
- [x] 5.7 Add unit or integration tests proving events fire once per action.
  (Unit: `manualController.test.ts` — one `params_changed` per action with
  all keys on one event, zero for no-op, unique ids across repeats. Flow
  level: Playwright macro-diff proof exercises single apply → single macro
  event feeding the panel.)

## 6. T6 - State Refactor Planning Gate

Write scope:

- `openspec/changes/session-collab-visibility/design.md`
- optional docs under `docs/`

Tasks:

- [x] 6.1 Document current state owners and target projection owner.
- [x] 6.2 Mark which stores remain runtime caches in first slice.
- [x] 6.3 Define migration order from `workingCopy`/`paramPanelState` to session
  projections.
- [x] 6.4 Add non-goal: no broad state rewrite before visibility events pass.

## 7. T7 - Verification

Tasks:

- [x] 7.1 Run `npm run typecheck`.
- [x] 7.2 Run `npm run test:unit`.
- [x] 7.3 Run targeted Playwright for bubble/activity/history-preview flows.
- [x] 7.4 Run existing manual-code/params/version-switch guard tests.
  (`manual-code-apply` + `genie` + `params` + `history` specs: 54 passed.
  Repairs made along the way: `.cm-ecky-kind` token drift in the syntax
  assertion, `Version title`/`Version name` aria-labels on CodeModal commit
  fields, close-on-commit (`commitManualVersionAndClose` in App), and the
  stall test now asserts hidden-ness since closed windows stay mounted.)
- [x] 7.5 Run `cd src-tauri && cargo check`.
- [x] 7.6 Update tasks as implementation completes.

## Parallel Plan

Suggested worker split:

- Worker A: T1 only.
- Worker B: T2 only, using stub event data until T1 integrates.
- Worker C: T3 only.
- Worker D: T4 only.
- Worker E: T5 only after T1 event types stabilize.
- Main thread: integrates A-D, then runs E, then T7.

Dependency order:

```text
T1 -> T5 -> T7
T2 -> T7
T3 -> T7
T4 -> T7
T6 runs in parallel with T1-T4
```
