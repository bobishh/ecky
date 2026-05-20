# Proposal: Sketch Preview Draft Lifecycle

## Intent

Make sketch preview a durable draft object instead of an ephemeral render result.

Current sketch preview renders an artifact bundle and pushes it into app state,
but it does not create a stable draft entity that can be saved, discarded, or
resumed later. That makes preview useful for inspection but weak as a work unit.

This change introduces a sketch draft lifecycle with a stable draft identity and
explicit save/discard actions. Preview reruns update the same draft scope, so
render does not create a new thread every time.

## Scope

- Add a stable sketch draft entity for preview sessions.
- Keep repeated preview renders on the same draft identity.
- Make sketch preview saveable into a persisted draft snapshot.
- Make sketch preview discardable without mutating committed history.
- Keep render preview and draft persistence separate from normal manual code
  versioning.

## Out of Scope

- Per-render thread creation.
- New sketch geometry semantics.
- Rewriting the sketch editor interaction model.
- Changing non-sketch manual code draft behavior.

## Expected Outcome

Sketch preview feels like a real draft workspace: render, inspect, save, or
discard without spawning a new thread for every preview pass.
