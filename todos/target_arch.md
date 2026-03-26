# Drydemacher Target Architecture

## Purpose

This file defines the target architecture for Drydemacher after MVP.

The goal is not aesthetic cleanup.
The goal is to make behavior explicit, preserve invariants, and reduce regression risk while keeping product velocity high.

## Core Architectural Principle

Drydemacher should be modeled as a small stateful system with explicit transitions.

Prefer:
- explicit phase/state models
- working copy vs persisted version separation
- boundary-only exception handling
- deterministic commit rules
- backend-owned context assembly
- UI components that derive from domain state

Avoid:
- boolean soup
- hidden coupling between UI state and domain state
- retry logic that mutates persistence implicitly
- frontend-built truth that diverges from backend truth
- exceptions as normal control flow

## Product Invariants

These must remain true at all times.

- A question does not mutate geometry.
- A generated design becomes a persisted version only after successful FreeCAD render.
- A failed retry remains only a working copy, not a saved version.
- Manual commit creates a new persisted version only after successful validation render.
- Param/control edits affect the active persisted version only when one is truly selected.
- If no persisted version is selected, edits belong to the working copy only.
- One user submission may have at most one active lifecycle pipeline.
- Render retries are internal repair transitions, not new user turns.
- Prompt context is assembled from current snapshot + rolling summary + recent dialogue + pinned references.
- The current version snapshot is canonical truth. Summaries are compression, not authority.

## Desired Domain Model

### 1. Session State

A single store/controller should own the session lifecycle.

Example shape:

```ts
interface SessionState {
  phase:
    | 'booting'
    | 'idle'
    | 'classifying'
    | 'answering'
    | 'generating'
    | 'rendering'
    | 'repairing'
    | 'error';
  threadId: string | null;
  selectedVersionId: string | null;
  workingCopy: WorkingCopy | null;
  lastError: string | null;
  request: RequestState | null;
}
```

### 2. Working Copy

Working copy must be a first-class concept.

```ts
interface WorkingCopy {
  title: string;
  versionName: string;
  macroCode: string;
  uiSpec: UiSpec;
  params: Record<string, unknown>;
  dirty: boolean;
  sourceVersionId: string | null;
}
```

Rules:
- `selectedVersionId` points to persisted history.
- `workingCopy` may diverge from selected version.
- unsaved generated retries update `workingCopy` only.
- successful generation commits `workingCopy` to history and returns a new selected version.

### 3. Request State

```ts
interface RequestState {
  id: string;
  originalPrompt: string;
  currentPrompt: string;
  attempt: number;
  maxAttempts: number;
  questionMode: boolean;
  screenshot: string | null;
  attachments: Attachment[];
}
```

This allows all retries and UI statuses to derive from one source.

## Target Module Layout

### Frontend

Current problem:
- `src/App.svelte` owns too much.

Target split:

- `src/lib/stores/sessionFlow.ts`
  - lifecycle state machine
  - submit/question/generate/render/repair/commit flow
- `src/lib/stores/workingCopy.ts`
  - current working copy and version selection
- `src/lib/stores/history.ts`
  - thread list, active thread, active version, reload/delete/fork behavior
- `src/lib/audio/microwave.ts`
  - hum, ding, mute, cleanup
- `src/lib/boot/restore.ts`
  - config/history/last session restore orchestration
- `src/App.svelte`
  - composition shell only

### Backend

Rust is already closer to target.

Target responsibilities:

- `lib.rs`
  - command boundaries only
  - orchestration glue only where needed
- `db.rs`
  - persistence only
- `llm.rs`
  - external model calls only
- `freecad.rs`
  - render boundary only
- `models.rs`
  - data contracts only
- optional future modules:
  - `context.rs` for prompt/context assembly
  - `thread_summary.rs` for summary/pinned-reference maintenance

## Target Lifecycle

### Question Flow

```text
idle
-> classifying
-> answering
-> persist question exchange
-> idle
```

### Design Flow

```text
idle
-> classifying
-> generating
-> render candidate
-> if success: commit version -> idle
-> if failure and retries remain: repairing -> generating
-> if failure exhausted: error with working copy preserved
```

### Manual Commit Flow

```text
idle
-> validate render
-> if success: commit new version
-> if failure: remain on working copy, do not persist
```

## What Must Leave App.svelte

These are the main extraction targets.

- request lifecycle orchestration
- retry bookkeeping
- working copy bookkeeping
- thread/version selection logic
- boot restore sequencing
- microwave audio lifecycle
- ECKY mode derivation from raw booleans

`App.svelte` should become mostly:
- layout
- component composition
- event wiring
- derived presentational props

## Error Handling Style

Drydemacher should treat errors as explicit states, not ambient accidents.

Target pattern:
- backend commands return typed success/failure boundaries
- frontend flow transitions to `error` or `repairing`
- no silent persistence in failure states
- no generic `try/catch` islands that also mutate unrelated UI state

Use `try/catch` at boundaries only:
- Tauri invoke
- Web Audio APIs
- file access
- external LLM / FreeCAD commands

Do not use `try/catch` as the primary orchestration model.

## Context Ownership

Backend should own prompt context assembly.

Frontend should send only:
- user prompt
- thread id
- working snapshot when unsaved
- attachments
- screenshot if available

Backend should assemble:
- canonical current snapshot
- working snapshot override when present
- thread summary
- recent dialogue
- pinned references

This prevents frontend/backend prompt drift.

## Refactor Order

### Phase 1: State Model

- [ ] Introduce one explicit session phase model.
- [ ] Remove generation-related boolean overlap where possible.
- [ ] Derive ECKY state from phase only.
- [ ] Derive microwave visibility from phase only.

### Phase 2: Working Copy Separation

- [ ] Extract working copy logic from `App.svelte` into a store/module.
- [ ] Ensure persisted version selection and working copy divergence are explicit.
- [ ] Prevent param/control saves from mutating history when no version is selected.

### Phase 3: Request Controller

- [ ] Move submit/classify/generate/render/repair/commit flow into `sessionFlow.ts`.
- [ ] Give each request a stable request id.
- [ ] Centralize retry transitions and terminal failure handling.

### Phase 4: Boot Restore

- [ ] Move config load, history load, and last-session restore into one boot coordinator.
- [ ] Make boot transitions linear and observable.

### Phase 5: Audio Extraction

- [ ] Move microwave hum/ding/mute logic into `audio/microwave.ts`.
- [ ] Expose simple interface: `start()`, `stop(success)`, `setMuted(bool)`.

### Phase 6: Backend Context Extraction (DONE)

- [x] Extract prompt/context assembly into a dedicated backend module (`context.rs`).
- [x] Keep `lib.rs` command handlers thinner.

### Phase 7: Warning Cleanup

- [ ] Fix `ParamPanel.svelte` captured-state warning.
- [ ] Fix interactive-div accessibility warnings in shell/history/window components.
- [ ] Keep this after flow refactor, not before.

## Definition Of Done

Drydemacher is at target architecture when:

- one explicit phase drives lifecycle behavior
- persisted history contains only valid committed versions
- working copy is explicit and durable
- retries are observable and never pollute version history
- frontend components mostly render derived state instead of owning orchestration
- prompt context has one source of truth
- app behavior can be explained as state transitions instead of incidental side effects

## Non-Goals

Not part of this refactor:
- redesigning the visual language
- changing the Tactical Midnight UI direction
- replacing ECKY
- introducing heavy state libraries unless they clearly reduce complexity
- solving every a11y warning before the flow model is fixed

## Short Version

Target Drydemacher shape:
- explicit state machine
- explicit working copy
- commit-on-success only
- backend-owned context
- thin UI shell
- boundary-only exceptions
