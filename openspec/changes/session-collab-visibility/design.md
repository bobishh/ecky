# Design: Session Collaboration Visibility

## Architecture

Target flow:

```text
user/agent/system command
  -> command handler
  -> SessionEvent append
  -> SessionStateComposer projections
  -> bubble / activity window / editor diff / preview modal / timeline
```

The event log is the source for visibility. Existing stores can remain as
runtime caches while projections are introduced.

## Session Event Model

First slice data model:

```ts
type SessionActor =
  | { kind: 'user'; id: string }
  | { kind: 'agent'; id: string; label: string }
  | { kind: 'system'; id: string };

type SessionEvent = {
  id: string;
  sessionId: string;
  threadId: string | null;
  versionId: string | null;
  actor: SessionActor;
  kind: SessionEventKind;
  title: string;
  summary: string;
  timestamp: number;
  severity: 'info' | 'success' | 'warning' | 'error' | 'question';
  artifacts?: SessionEventArtifact[];
  diffs?: SessionEventDiff[];
  raw?: unknown;
};
```

First event kinds:

- `agent_action_started`
- `agent_action_finished`
- `macro_patch_proposed`
- `macro_patch_applied`
- `params_changed`
- `render_started`
- `render_succeeded`
- `render_failed`
- `validation_reported`
- `preview_updated`
- `version_committed`
- `user_decision`

## Composer

Add a pure composer module:

```ts
composeSessionActivity(sessionEvents, activeThreadId, activeVersionId)
composeBubbleEvent(activity)
composeCodeDiffView(activity, selectedCode)
composePreviewActivity(activity)
```

The composer keeps UI decisions deterministic and testable:

- which event gets bubble focus.
- whether bubble is compact.
- what opens on click.
- which diff is the latest macro diff.
- which preview/validation artifacts are relevant.

## Bubble and Activity View

`VertexGenie.svelte` remains visual shell, but receives an action callback and
never owns the full log.

Behavior:

- bubble text can stay compact.
- bubble click opens session activity.
- copy/dismiss buttons keep working.
- full text, raw validation reports, diffs, screenshots, and preview thumbnails
  live in activity view.

Activity view can start as a `Window.svelte` modal/window, not a new route:

- event list on left/top.
- selected event details on right/bottom.
- artifacts section for previews/screenshots.
- diff section for macro/params.
- raw error/details section.

## Code Diff

`CodePanel.svelte` uses CodeMirror for editable current code. First slice avoids
deep editor extension risk:

- current editor stays editable.
- below editor, show `LAST MACRO DIFF` section.
- display old/new snippets or unified diff in read-only CodeMirror/basic code
  block.
- highlight changed lines from the latest macro event.

If CodeMirror merge/diff extension is already easy to add safely, use it behind
the same `CodeDiffView` API. Otherwise keep a read-only diff panel.

## Preview and Validation

Preview events attach:

- image data if present.
- STL/STEP paths if present.
- model id/content hash if present.
- validation report id if related.

Activity view opens preview details:

- image thumbnail or current viewer capture.
- artifact names/paths.
- validation summary.
- raw issue list.

Bubble click routes to the activity event, not directly to export or random
viewport state.

## Integration Strategy

Do not migrate all stores at once. Add event emission at command boundaries:

- agent draft preview event listener.
- manual code apply/commit.
- param apply/commit.
- render success/failure.
- validation feedback.
- version commit/delete.
- preview persistence.

Existing stores continue to update. Events are visibility/audit layer first.
After proof, state ownership can move from stores to projections.

## Current State Faults Found

### Param commit can overwrite macro code

Observed owners:

- `src/lib/controllers/manualController.ts` owns `handleParamChange()`.
- `src/App.svelte` owns `handleParamPanelCommit()` and panel `onspecchange`.
- `src-tauri/src/mcp/handlers.rs` owns MCP macro/params preview merges.
- `workingCopy` and `paramPanelState` both cache draft state.

Failure path:

- `handleParamChange(nextParams, null, true)` builds a committed design from
  `workingCopy.macroCode` plus merged params.
- If current code draft has not reached `workingCopy.macroCode`, param commit
  reloads the working copy from an older mutable snapshot.
- MCP preview handlers have separate `base_design.initial_params + patch`
  logic, so agent patches and panel params do not share one precedence rule.

Target rule:

- `ComposedSessionState` owns commit/render inputs.
- Code draft and param draft are independent inputs.
- Params commit consumes composed snapshot, not raw `workingCopy`.
- Backend preview receives explicit `baseParams`, `currentParams`, `agentPatch`,
  and `macroCode` fields instead of re-deriving precedence.

### Project preview cache can stick on `NO PREVIEW`

Observed owners:

- `src/lib/ProjectSwitcher.svelte` owns card cache and visible-card warmup.
- `src/lib/projectPreview.ts` owns `selectThreadPreviewImage()`.
- `src/lib/stores/history.ts` owns thread/version load and message paging.

Failure path:

- `ProjectSwitcher` listens for `ecky:version-preview-updated`, but no emitter
  was found in the repo.
- Card cache keys include latest version id. If preview fetch stores null for
  that id, the card can stay `NO PREVIEW`.
- Opening/loading a model updates active viewer/runtime state, but does not
  publish fresh preview data back to card cache.
- Preview fetch uses bounded page sizes, so deeper preview-bearing messages can
  be missed unless paging happens.

Target rule:

- Successful preview persistence/load emits one preview update event with
  thread id, message id, image data/path, and artifact metadata.
- Project cards update cache from that event.
- Card warmup pages until first renderable preview or an explicit bounded
  search limit, tested as product behavior.

## Parallel Boundaries

Workers should not edit the same files unless the integration owner approves.

- T1 owns new session activity modules and tests.
- T2 owns `VertexGenie.svelte` and activity window shell.
- T3 owns `CodePanel.svelte` and diff helpers.
- T4 owns preview/validation artifact rendering.
- T5 owns event emission at command boundaries.
- T6 owns e2e tests and integration wiring.
