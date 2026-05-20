# Design: Sketch Preview Draft Lifecycle

## Model

Introduce a stable sketch draft record that captures:

- draft id
- scope id
- source snapshot
- artifact bundle
- last rendered timestamp

Preview rerenders update the existing draft record in place when the source
identity has not changed.

## UI Flow

- `Preview` creates or refreshes the active sketch draft scope.
- `Save Draft` persists the current draft snapshot for restore.
- `Save Draft` can optionally fork to a fresh draft scope.
- `Discard Draft` deletes the draft snapshot and clears the active preview state.
- `Preview` after discard starts a new draft scope only if the user edits again.

## Boundaries

- Preview rendering stays local and repeatable.
- Persistence happens only on explicit save to backend draft storage.
- Draft deletion does not mutate committed history.
- A preview draft is not a new thread per render.

## Risks

- Need deterministic identity for “same draft”.
- Need a clear transition from sketch draft to durable thread/version record.
- Need browser proof that rerender updates draft instead of duplicating it.
