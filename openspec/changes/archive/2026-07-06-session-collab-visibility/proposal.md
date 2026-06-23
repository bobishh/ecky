# Proposal: Session Collaboration Visibility

## Intent

Make Ecky behave like a pair-programming CAD room: user, agents, renders,
validations, previews, macro edits, parameter edits, and commits all act inside
one visible session.

Current app state is split across active version, working copy, param panel,
session runtime, history, preview cache, agent state, and transient bubbles. That
causes invisible mutation paths: agent work can change macro/params/render state
without a durable user-visible trail, bubble text truncates important details,
and preview/code actions are hard to inspect after the fact.

## Scope

- Add a session event log and projections for user/agent/system actions.
- Make agent actions visible in the advisor bubble and expandable in a modal.
- Add a session activity window/dialogue extension for full event details and
  preview artifacts.
- Add macro diff display next to or below the CodeMirror editor.
- Show parameter diffs, validation reports, render outcomes, and version commits
  as first-class session events.
- Keep existing stores alive during migration; route new behavior through
  composed projections.

## Out of Scope

- Replacing all app stores in one pass.
- Rewriting the whole UI shell.
- Removing history/version persistence.
- Implementing multiplayer networking.
- Building a full CRDT editor.
- Changing CAD macro syntax.

## Approach

Use one umbrella change with parallel workstreams:

- T1: session event model and state composer.
- T2: visible agent/action feed and bubble expansion.
- T3: CodeMirror macro diff panel.
- T4: preview and validation extended modal/window.
- T5: event-producing command integration.
- T6: e2e proof gates and integration cleanup.

Each worker gets a disjoint write scope. Main thread integrates through OpenSpec
tasks and keeps user-visible behavior as acceptance proof.

## Product Direction

Session is the shared room. User owns it. Agents are participants. System
processes are participants. Every meaningful action is appended as an event.

Examples:

- `AgentProposedMacroPatch`
- `AgentChangedParams`
- `RenderStarted`
- `RenderSucceeded`
- `ValidationReported`
- `PreviewUpdated`
- `VersionCommitted`
- `UserAcceptedPatch`
- `UserRejectedPatch`

UI reads projections, not hidden owner-specific state:

- advisor bubble: newest important event, never the only copy.
- session activity modal/window: full log, diffs, raw errors, preview thumbnails.
- code modal: current code plus last macro diff.
- param panel: changed controls and old/new values.
- dialogue: agent-visible work and user decisions.

## Proof Gates

- Every agent/system action with user-visible consequences creates a session
  event.
- Bubble click opens the full session activity view.
- Long agent text is visible in the modal/window without truncation.
- Macro change opens or updates CodePanel with a diff label and old/new view.
- Parameter change events show old/new values.
- Validation report events show raw issues and related preview artifact.
- Preview event opens an extended preview view.
- Commit event appears in activity and version timeline.
- Existing manual code/params flows still pass.
- `npm run typecheck`, targeted Playwright, and `cd src-tauri && cargo check`
  pass before completion.
