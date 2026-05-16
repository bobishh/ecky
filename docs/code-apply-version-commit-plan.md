# Code Apply And Version Commit Plan

Date: 2026-05-15
Status: Plan

## Goal

Code edits behave like parameter edits:

- Editing code creates local draft only.
- Apply renders draft into working preview and active runtime, but creates no new history version.
- Commit remains separate user action. Commit snapshots code plus parameters exactly as they are at commit time.
- Version name/title editable before commit.
- Existing manual/version machinery reused where possible.

## Current Shape

Parameter path already has draft/apply semantics:

- `ParamPanel.svelte` owns local `localParams`.
- `APPLY` calls `handleParamChange(localParams)`.
- `handleParamChange` renders with `renderModel`, updates viewer/session/runtime, then persists active message parameters/runtime through `updateParameters`, `updatePostProcessing`, and `updateVersionRuntime` when requested.
- No new history message/version gets created by param apply.

Code path lacks same split:

- `CodeModal.svelte` edits code and exposes `COMMIT AS NEW VERSION`.
- `commitManualVersion` reconciles controls, renders, then calls `addManualVersion`.
- `addManualVersion` creates new `Message` row and history version immediately.
- Current commit path uses reconciled params from macro parse. It can miss final parameter tweaks user made after editing.

## Target UX

Code modal footer becomes:

- `APPLY`: render edited code against current params. No new version. Same mental model as params panel `APPLY`.
- `COMMIT VERSION`: opens/uses small commit controls with editable `title` and `versionName`.
- `FORK TO NEW THREAD`: remains separate action, but also snapshots current params.

Commit controls:

- Default `title`: current working copy title.
- Default `versionName`: current working copy version name or generated next label.
- User can change both before commit.
- Commit button disabled while apply/commit running.
- Dirty indicator shown when code draft differs from working copy macro or params draft differs from committed active version.

Language:

- Prefer `APPLY CODE` only if plain `APPLY` conflicts with nearby controls.
- Prefer `COMMIT VERSION` over `COMMIT AS NEW VERSION`.
- Avoid implying apply creates history.

## Behavioral Contract

Apply code:

1. Read edited code from `CodeModal`.
2. Read params from `paramPanelState.params` at click time.
3. Reconcile controls from edited code, preserving compatible current params.
4. Render via existing `renderModel`.
5. Inspect runtime via `inspectRuntimeBundle`.
6. Save/ensure manifest as current code path does.
7. Update `session`, `workingCopy`, `paramPanelState`, `activeVersionId` runtime payload.
8. Persist active message runtime/source/params only if existing active version should track applied working draft.
9. Create no `addManualVersion` message.

Commit version:

1. Require successful render/apply or render during commit if unapplied draft exists.
2. Read edited code, current `paramPanelState.params`, title, and version name at click time.
3. Call reused manual version service with those exact values.
4. Create one new history `Message`.
5. Set active version to new message.
6. Persist last session snapshot with committed code, params, artifact bundle, and manifest.

Failure:

- Apply render failure leaves last good preview active.
- Raw backend/provider error displayed, no generic "check API key" copy.
- Commit failure creates no version.
- Pending state visible for apply and commit.

## Implementation Plan

### 1. Outer BDD: code apply does not create version

Add Playwright spec under `e2e/`:

Given existing generated version and code modal open
When user edits macro code and clicks apply
Then `render_model` runs with edited code and current params
And `add_manual_version` does not run
And viewer/session shows new preview state
And active version count stays unchanged

Failure/pending check:

Given edited code render fails
When user clicks apply
Then error body appears in modal/session
And last good preview remains
And no version gets created

### 2. Inner unit: code draft apply controller

Extract controller function from `commitManualVersion`:

- `applyManualCodeDraft(editedCode, options)`
- Inputs use camelCase in TS:
  - `editedCode`
  - `parameters`
  - `persistToActiveVersion`
  - `messageId`
- Return:
  - `design`
  - `artifactBundle`
  - `modelManifest`
  - `parserMatched`

Reuse existing pieces:

- `reconcileManualControls`
- `renderModel`
- `inspectRuntimeBundle`
- `getRenderableRuntimeBundle`
- `getModelManifest`
- `ensureSemanticManifest`
- `saveModelManifest`
- `updateVersionRuntime`
- `updateParameters`
- `updatePostProcessing`

Do not call:

- `addManualVersion`
- `rememberCommittedVersionMessage`
- `refreshHistory` unless active message persistence changes thread cards.

### 3. Commit snapshots current params

Change `commitManualVersion` so parameters are not only derived from macro parse.

Rule:

- Parse macro controls to update `uiSpec`.
- Current `paramPanelState.params` wins for keys still valid after reconciliation.
- Missing keys use parsed defaults/fallbacks.
- Commit payload `parameters` equals final visible params at click time.

Add unit tests around reconciliation:

- Existing param value survives code parse when key remains.
- Removed key is dropped.
- Added key gets parsed default.
- Commit uses `versionName` supplied by user.

### 4. Add title/version inputs

Update `CodeModal.svelte`:

- Local `draftTitle`.
- Local `draftVersionName`.
- Prop defaults from `workingCopy`.
- Apply callback receives code only.
- Commit callback receives `{ code, title, versionName }`.
- Keep square Tactical Midnight styling and bounded footer overflow.

Update caller wiring in `App.svelte` or extracted modal host:

- Pass working copy title/version name.
- Wire `onApply` to `applyManualCodeDraft`.
- Wire `onCommit` to changed `commitManualVersion`.

### 5. Backend boundary

Likely no new Rust command required.

If source persistence on apply needs backend storage, add narrow command:

- TS invokes camelCase: `updateVersionSource(messageId, designOutput)`
- Rust input struct uses `#[serde(rename_all = "camelCase")]`
- Rust fields stay `snake_case`.

Prefer existing `update_parameters`, `update_post_processing`, `update_version_runtime`, and `add_manual_version` if enough.

### 6. Rename mental model

UI copy:

- Code modal primary apply: `APPLY`
- History commit action: `COMMIT VERSION`
- Status after apply: `Code applied. Commit version to save history.`
- Status after commit: `Version committed.`

No separate agent status bar. No terminal output in app logs.

## Data Rules

Working draft:

- Lives in `workingCopy` plus modal local code.
- May have runtime artifact bundle from last apply.
- Not durable history by itself.

Active version:

- Existing message remains active until commit creates new message.
- Runtime payload may update after apply if current behavior must keep reload stable.
- Version count does not increase on apply.

Committed version:

- New message via `addManualVersion`.
- Contains final code.
- Contains final current params.
- Contains editable `title` and `versionName`.
- Contains artifact bundle/manifest from latest apply or commit render.

## Test Matrix

Playwright:

- Code apply happy path.
- Code apply render failure.
- Commit after parameter tweak includes latest params.
- Commit title/version name edits appear in history/timeline.
- Fork uses latest params and new thread.

Unit:

- Reconcile current params with parsed code controls.
- Apply controller does not call version creation dependency.
- Commit controller calls version creation once with provided name.
- Save manifest called with message id only on persisted path.

Rust:

- If backend command added, `cargo check`.
- Add command tests only if DB/source persistence changes.

## Rollout Slices

1. Add failing Playwright spec for apply/no-version.
2. Extract apply controller from manual commit path.
3. Wire `CodeModal` apply button.
4. Add commit metadata inputs and callback payload.
5. Change commit to snapshot current params.
6. Add failure/pending BDD coverage.
7. Run:
   - `npx playwright test e2e/<new-spec>.ts`
   - `npm run test:unit`
   - `cd src-tauri && cargo check` if Rust touched

## Open Decisions

- Should apply persist edited source into existing active message, or keep source only in working draft until commit?
- Should applying code update current version name in working copy, or should version name only change during commit?
- Should commit require prior successful apply, or auto-render unapplied draft during commit?

Recommended defaults:

- Keep source draft-only until commit.
- Version name changes only during commit.
- Commit auto-renders if draft is unapplied, then commits exact render output.
