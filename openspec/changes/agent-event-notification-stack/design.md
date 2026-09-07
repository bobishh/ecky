# Design: Agent Event Notification Stack

## Current Flow And Loss Points

```text
MCP/runtime action
  -> mutable ThreadAgentState snapshot
  -> one-second frontend poll
  -> ordered if/else bubble resolver
  -> one bubble string

MCP trace event
  -> formatted app log only

selected manual/render/preview action
  -> frontend-only SessionEvent store
  -> activity window
```

Loss occurs before rendering. Polling observes state, not transitions. The
single-winner resolver then removes concurrency. Synthesizing an activity event
from the final bubble cannot reconstruct intermediate actions.

## Target Flow

```text
MCP/runtime/manual/system action from any thread
  -> typed AgentActivityEvent append
  -> backend global journal + Tauri push
  -> cursor catch-up / dedupe
  -> one app-global frontend activity store
       -> durable activity projection
       -> notification queue projection
            -> global AgentNotificationCenter
```

## Decision 1: Event Contract, Not Snapshot Diffing

Add a Rust `AgentActivityEvent` boundary struct. Rust fields remain
`snake_case`; the struct uses `#[serde(rename_all = "camelCase")]`. Generated
TypeScript remains `camelCase`.

Required fields:

```ts
type AgentActivityEvent = {
  eventId: string;
  cursor: number;
  sessionId: string;
  threadId: string | null;
  messageId: string | null;
  versionId: string | null;
  actor: { kind: 'agent' | 'system'; id: string; label: string };
  kind: AgentActivityEventKind;
  lifecycleKey: string | null;
  phase: string | null;
  summary: string;
  detail: string | null;
  severity: 'info' | 'success' | 'warning' | 'error' | 'question';
  state: 'active' | 'resolved' | 'failed' | 'canceled';
  requiresAttention: boolean;
  occurredAt: number;
  raw: unknown | null;
};
```

`cursor` is monotonic across the app-global journal, including events from
different threads and sessions. `eventId` is globally unique and is the
dedupe/dismiss key. `lifecycleKey` is namespaced by session and links progress
transitions belonging to one logical operation. `detail` and `raw` preserve
provider/backend facts; notification copy uses `summary` only.

Remove `ThreadAgentState` from the frontend runtime after event projections own
its consumers. Mascot animation, connection controls, busy state, phase,
attention, and labels derive from the latest journaled lifecycle events. Initial
load, reconnect, and gap repair use `getAgentActivity({ afterCursor })`; there is
no `getThreadAgentState` polling or snapshot fallback.

## Decision 2: One Journal-And-Emit Boundary

Replace log-only trace handling with one backend service that:

1. validates and appends the typed event through the canonical backend store;
2. assigns global cursor and event identity;
3. emits `agent-activity-event` through Tauri;
4. logs only a bounded structured summary, never terminal output.

Every state transition needed by a former `ThreadAgentState` consumer MUST emit
an event. Missing projection data is a contract/test failure, not a reason to
query the old snapshot path.

Existing MCP `push_trace_event` call sites adapt into this service. Runtime
connection, attention, prompt, cancellation, and failure transitions use the
same path. Existing preview/render/manual `SessionEvent` producers either
consume the backend event or map to the unified frontend type once; they must
not create a duplicate synthetic bubble event.

The backend exposes one app-global catch-up command using camelCase invoke
arguments:

```ts
getAgentActivity({ afterCursor })
```

It returns globally ordered events plus the latest cursor. Frontend installs one
Tauri listener at app bootstrap, fetches catch-up second, then deduplicates by
`eventId`. This closes the subscribe/fetch race. Thread filtering is forbidden
at ingestion. Persistence uses existing backend services; frontend and tests
never write SQLite directly.

## Decision 3: Complete Journal, Bounded Notification Projection

One global `SessionEvent` activity store is the audit surface. A pure mapper converts
`AgentActivityEvent` without losing actor, raw detail, lifecycle identity, or
timestamps. Activity and notification ordering use the backend global cursor;
local events receive cursors through the same backend append boundary.
Ephemeral frontend-only interaction state such as onboarding controls or a
queue submission error is not forged into that journal. It may render as one
local interaction card in the same center, outside cursor ordering and activity.

`projectAgentNotifications(events, now, visibilityState)` returns:

- up to four visible cards;
- FIFO waiting cards;
- lifecycle updates folded into the visible card with the same
  `lifecycleKey`;
- expiry metadata independent from render time;
- no notification for transport-only noise.

Folding affects only cards. Activity keeps every transition. Queue overflow
never evicts an unseen card: expiry or dismissal promotes the next FIFO item.

## Decision 4: Explicit Lifetime State Machine

```text
queued -> visible(active) -> visible(resolved) -> expired
                    |                |
                    |                -> dismissed
                    -> failed/sticky -> resolved or dismissed
                    -> question/sticky -> answered or dismissed
```

- Active: no deadline.
- Resolved info/success: 8,000 ms visible-time deadline.
- Resolved warning: 12,000 ms visible-time deadline.
- Error/question/action required: no automatic deadline.
- Minimum exposure: every promoted card receives at least 2,000 ms visible time,
  even if it resolved while queued.
- Hover or focus within a card pauses that card.
- `document.visibilityState !== 'visible'` pauses all cards.
- Updating an active card does not reset elapsed visible time. Transition to a
  terminal state starts the terminal TTL exactly once.
- Dismissal and expiry change notification projection only. They never delete
  activity records.

Timers live in a dedicated notification store/runtime, not `VertexGenie`.
Timer tests use an injected clock.

## Decision 5: Notification Center Is Global And Separate From VertexGenie

`VertexGenie.svelte` is mascot rendering only. Remove `bubble`, notification,
copy, dismiss, activity-open, relay-card, and notification lifecycle props from
its contract. It neither subscribes to nor renders agent notifications.

Mascot identity belongs to persisted `Thread.genieTraits`. An active thread keeps
the same seed across render, preview, artifact, and version changes. A
model-derived seed is fallback only when persisted thread traits are absent.
Settings rerolls key local overrides by stable thread identity, never by model
artifact identity.

Add one `AgentNotificationCenter.svelte` at the app shell level. It subscribes
directly to the app-global notification store and renders one overflow-hidden
stack container with one card per visible notification. `App.svelte` mounts the
center once; it does not calculate or pass notification arrays. No status bar is
added. No terminal output is copied.

The shell positions this center inside the Ecky layer at the former speech
bubble locus. No top-right toast lane remains. Local onboarding, confirmation,
queue-error, and action controls publish one frontend-only interaction card to
the center; they never render a second bubble and never impersonate a backend
cursor event.

Each card contains thread label, actor/relay attribution, compact summary,
age/state marker, copy, dismiss where allowed, and exact-event click callback.
Oldest card renders above newest card. Cards belonging to the active thread use
a separate inner bronze accent and full text contrast. Result border color stays
semantic: green for success, bronze for warning, red for error. Draft preview
feedback records `passed` as resolved success, `warning` as resolved warning,
`failed` as failed error, and `checking` as active info. Other-thread cards stay
visible with muted border/contrast; they are never filtered. Changing active
thread only recomputes its accent. Tactical Midnight colors, square borders,
`--primary` and `--secondary` accents remain. Stack and all major containers use
`overflow: hidden`; long card bodies clamp, while activity detail remains full.

Non-urgent additions use one polite live region. New errors/questions use an
assertive announcement once, not on every elapsed-time update. Keyboard focus
can reach every card and pauses its TTL.

## Decision 6: Source Priority Becomes Event Policy

Delete the global source-precedence `if / else` as the visibility mechanism.
Priority controls stickiness and presentation, not existence:

- prompt/confirm/error: sticky and attention-styled;
- active tool/agent work: non-expiring progress;
- success/info/warning: timed after terminal state;
- onboarding and user choices: separate lifecycle events, sticky while active;
- identical text from different `eventId` values: distinct cards.

Mascot mode may still derive from current state priority outside
`VertexGenie`. That visual mode must not suppress notification events.

## Failure Handling

- Cursor gap: fetch from last contiguous global cursor before showing later
  cards.
- Duplicate push/catch-up event: ignore by `eventId`.
- Unknown event kind: retain in activity as `agent_action_finished`, show raw
  kind in detail, and use info presentation.
- Catch-up failure: show raw failure as sticky system error while preserving
  already received events; retry with bounded backoff.
- Missing thread association: retain under session activity and mark thread as
  null; do not silently discard.
- Explicit MCP `session_activity_set` and `session_activity_clear` calls are
  thread-scoped and resolve their thread from the current bound session target.
  Reject targetless calls with a validation error. App-global system events use
  the backend journal boundary directly instead of masquerading as agent work.

## Migration

1. Introduce global contract, journal, subscription, catch-up, and Rust tests
   without changing UI.
2. Add frontend ingestion/dedupe and prove complete ordered activity.
3. Add pure notification lifecycle store and fake-clock tests.
4. Add one shell-level `AgentNotificationCenter` subscribed directly to the
   global store.
5. Strip bubble/notification responsibilities from `VertexGenie`.
6. Remove `bubbleSessionEvent` synthesis and text-keyed dismissal.
7. Remove bubble visibility dependence on `resolveGenieBubblePresentation`;
   retain only reusable presentation helpers.
8. Remove the `getThreadAgentState` interval, frontend invocation, snapshot
   resolver, and command/contract when no non-UI consumer remains. Do not keep a
   bootstrap or recovery fallback; bootstrap and recovery use event catch-up.

## Rejected Paths

- Diffing consecutive polled snapshots: cannot recover transitions between
  polls.
- Increasing polling frequency: adds load and still loses bursts.
- Keeping snapshot polling as fallback: creates two authorities, masks missing
  events, and makes recovery behavior differ from normal behavior.
- Per-thread listeners or stores: fragment ordering and hide background-thread
  work.
- Passing notifications through `App.svelte` into `VertexGenie`: couples mascot
  rendering to global event ownership and recreates the single-thread funnel.
- Using terminal output as event source: noisy, untyped, leaks interactive
  terminal content into wrong UI.
- Rendering unbounded cards: causes viewport obstruction and jitter.
- Replacing the visible card on priority: repeats current data-loss defect.
- Clearing activity when TTL expires: destroys auditability.
