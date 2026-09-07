# Tasks: Agent Event Notification Stack

## 1. Outer Red: Complete Event Visibility

- [x] 1.1 Add Playwright acceptance fixture emitting six globally ordered agent events
  faster than the current poll interval.
- [x] 1.2 Assert current UI fails because activity misses transitions and only
  one bubble renders.
- [x] 1.3 Add pending/error acceptance scenarios before production changes:
  pending card stays; raw error remains inspectable.

## 2. Backend Event Contract

- [x] 2.1 Add Rust `AgentActivityEvent`, actor, kind, severity, and lifecycle
  state contracts with `#[serde(rename_all = "camelCase")]`.
- [x] 2.2 Add serialization tests proving camelCase output and raw error detail.
- [x] 2.3 Regenerate TypeScript contracts; frontend fields remain camelCase.
- [x] 2.4 Add backend-owned app-global monotonic cursor assignment and stable
  event IDs.

## 3. Journal, Push, And Catch-Up

- [x] 3.1 Add failing Rust tests for globally ordered append across threads,
  cursor gaps, and duplicate prevention.
- [x] 3.2 Implement canonical journal-and-emit service through existing backend
  persistence boundaries; do not write SQLite outside those services.
- [x] 3.3 Route MCP trace events through the typed service.
- [x] 3.4 Route runtime connection, attention, prompt, cancel, disconnect, and
  failure transitions through the same service.
- [x] 3.5 Add `get_agent_activity(after_cursor)` command and camelCase frontend
  invocation; catch-up MUST include all threads.
- [x] 3.6 Prove subscribe-first/catch-up-second race handling with no gaps or
  duplicates.
- [x] 3.7 Keep raw terminal stdout/stderr only in dedicated terminal snapshots.
- [x] 3.8 Emit typed events for every phase, busy, attention, connection, actor,
  and label transition currently supplied by `ThreadAgentState`.

## 4. Unified Frontend Activity

- [x] 4.1 Add failing unit tests mapping every agent event kind into
  `SessionEvent` without losing actor, lifecycle, timestamp, detail, or raw body.
- [x] 4.2 Add one app-global ingestion store with `eventId` dedupe and
  cursor-gap recovery.
- [x] 4.3 Merge backend agent events with existing manual/render/preview events
  once; eliminate duplicate event production.
- [x] 4.4 Remove synthetic `bubbleSessionEvent` as an activity source.
- [x] 4.5 Prove activity retains expired and dismissed notifications.
- [x] 4.6 Derive mascot, connection controls, busy, phase, and attention state
  from the global event store.
- [x] 4.7 Delete the one-second `getThreadAgentState` poll and frontend snapshot
  dependency. Delete the command/contract if no remaining consumer exists; do
  not retain a bootstrap/recovery fallback.

## 5. Notification Lifecycle Inner Loop

- [x] 5.1 Add fake-clock unit tests for four-card capacity, FIFO promotion, and
  stable oldest-to-newest order.
- [x] 5.2 Add red tests for active/no-TTL, info/success 8-second TTL, warning
  12-second TTL, and sticky error/question/action states.
- [x] 5.3 Add red tests for 2-second minimum exposure, hover/focus pause, hidden
  document pause, and terminal-state TTL start.
- [x] 5.4 Add red tests proving dismissal uses `eventId` and does not suppress a
  later identical message.
- [x] 5.5 Implement pure projection plus timer runtime with injected clock.
- [x] 5.6 Fold lifecycle updates into one card while retaining each source event
  in activity.

## 6. Global Notification Center

- [x] 6.1 Add one shell-level `AgentNotificationCenter` subscribed directly to
  the app-global store; do not pass notification data through `App.svelte`.
- [x] 6.2 Remove all bubble and notification props/rendering from `VertexGenie`;
  keep it mascot-only.
- [x] 6.3 Render up to four square Tactical Midnight cards in one vertical,
  overflow-hidden stack; oldest top, newest bottom.
- [x] 6.4 Show thread label, actor/relay attribution, state, compact summary,
  copy, and dismiss without exposing terminal output.
- [x] 6.5 Highlight active-thread cards strongly; keep other-thread cards visible
  and muted. Thread switching changes styling only.
- [x] 6.6 Make card click open its exact activity event without implicitly
  switching the active design thread.
- [x] 6.7 Add polite live-region behavior for normal events and one-shot assertive
  announcements for errors/questions.
- [x] 6.8 Preserve mascot animation and remove visibility dependence on the
  single-winner resolver.

## 7. Outer Green And Failure Proof

- [x] 7.1 Pass six-event burst scenario: all events in activity, all cards shown
  through FIFO promotion, no silent overflow loss.
- [x] 7.2 Pass concurrent cross-thread message scenario in one center: both
  visible, active-thread card highlighted, background-thread card retained.
- [x] 7.3 Pass completed-card expiry scenario and confirm activity detail remains.
- [x] 7.4 Pass pending scenario: active notification survives beyond timed TTL.
- [x] 7.5 Pass error scenario: sticky card and raw backend/provider body.
- [x] 7.6 Pass reload/listener-gap catch-up scenario without duplicates.
- [x] 7.7 Prove initial load and reconnect reconstruct current state through
  cursor catch-up with zero `getThreadAgentState` calls.

## 8. Verification

- [x] 8.1 Run focused frontend unit tests after each inner green step.
- [x] 8.2 Run targeted Playwright agent notification and activity specs.
- [x] 8.3 Run `npm run typecheck` and relevant existing genie/dialogue/manual flow
  regressions.
- [x] 8.4 Run `cd src-tauri && cargo check` and focused Rust tests.
- [x] 8.5 Run `openspec validate agent-event-notification-stack --strict`.
- [x] 8.6 Confirm no source-control stage or commit occurred without request.
- [x] 8.7 Reject targetless MCP session activity and preserve bound thread attribution.
- [x] 8.8 Keep mascot seed bound to persisted thread traits across model versions; retain model-derived fallback for legacy threads.
- [x] 8.9 Map draft preview feedback state to matching notification severity and result border tone.
