# Design: Ecky-Owned Provider Conversations

## Boundaries

Three layers stay separate:

1. Provider adapter boundary: external lifecycle and turns. Codex app-server is
   adapter one. Future Claude Code adapter implements equivalent start/resume/read/
   send/steer/stop behavior or reports unsupported capabilities.
2. `agent_thread_bindings` + lineage + `agent_provider_messages` +
   `agent_prompt_queue`: provider-neutral execution cursor, durable transcript, and FIFO.
3. Dialogue: selects global route from Settings and renders the current Ecky thread.

No frontend code lists provider-global history.

## Settings Contract

`Config.connectionType` values:

- `api_key`
- `mcp`
- `provider:codex`

Prefix preserves provider mode while suffix selects adapter. UI renders `PROVIDER`
as third connection type and `CODEX` as its current provider choice.

## Ownership and Lazy Creation

`agent_thread_bindings` remains keyed by Ecky thread id and uniquely constrains
`(provider, external_thread_id)`. It stores the current provider execution cursor,
not conversation authority. `agent_thread_binding_lineage` records historical cursors
only when a supported provider lifecycle explicitly replaces one; an active-writer
conflict never changes this binding.
First provider-mode submit executes:

1. read existing provider binding;
2. if absent, export/resolve the canonical thread-source folder and canonical handoff;
3. call Codex `thread/start` with non-ephemeral persistence, bootstrap, and required
   `ecky_provider_mcp` Streamable HTTP config;
4. persist returned id under the Ecky thread;
5. name external thread after Ecky title;
6. enqueue submitted prompt and dispatch FIFO.

Concurrent first sends are serialized. If binding persistence fails after start, Ecky
deletes only the just-created unbound external thread to avoid junk.

Opening provider-mode Dialogue reads Ecky's durable transcript and queue only. It
never resumes a provider writer, creates a thread, or calls `thread/list`. A
read-only background `thread/turns/list` backfill may reconcile finished turns into
Ecky without delaying Dialogue.

If delivery finds that another Codex client still owns the stored writer, Ecky
backfills readable finished turns, retains the current binding and external id, and
keeps the same queued prompt at the FIFO head with the raw provider error. The
supervisor retries that head on the existing delayed schedule after the writer may
have become available. Ecky does not create a replacement thread, unsubscribe, kill,
or retry in a tight loop. The installed Codex 0.153.4 app-server protocol exposes no
writer release or claim-transfer method; `thread/unsubscribe` only changes this
client's subscription and is not a takeover path.

## Prompt Bootstrap and Cross-Mode Handoff

At thread/start and each process-generation resume, developer instructions contain:

- Ecky identity and exact Ecky thread id/title;
- canonical project-mirror cwd, `model.ecky`/manifest paths, and live
  `ecky_provider_mcp` endpoint;
- inspect→validate→preview→verify workflow;
- canonical Ecky `THREAD SUMMARY`, `RECENT DIALOGUE`, design digest, and artifact
  digest from the existing context assembler.

After successful read-only backfill, Ecky stores normalized finished Codex
user/assistant messages plus user attachment metadata and builds a bounded handoff from canonical Ecky messages
plus recent provider dialogue. API and MCP already consume that canonical summary.
Provider compaction or a supported cursor replacement cannot erase Ecky's finished transcript.
The same transcript transaction advances the owning Ecky thread `updated_at`.
Its first non-empty provider user message replaces only the default
`Untitled design` title with the first 80 characters of normalized prompt text.
Manual titles remain authoritative. Schema initialization backfills these projections
for provider messages persisted before this invariant existed. History classifies
reusable blanks inside its bounded list query using indexed content existence checks;
it performs no per-thread follow-up reads.

## History and Runtime

Newest transcript page contains 30 locally persisted messages. `SHOW OLDER MESSAGES`
reads opaque-cursor pages from Ecky SQLite, prepends by stable item id, and preserves
scroll anchor. Provider I/O never sits on this UI path. Background provider backfill
is cursor paged and writes finished messages incrementally. Provider turns are
normalized oldest-first; user items precede assistant items inside a turn even when
timestamps collide. `agent_provider_messages.attachments_json` preserves prepared
local image paths or provider-backed inline image data, including completed Codex
`imageGeneration` outputs and any image content returned by completed Ecky sketch
MCP tools. Completed Ecky sketch tool calls also persist a concise assistant result
with the `SketchDocument` identity and sketch count; they remain evidence and do not
replace canonical `.ecky` source. Backfill updates missing
attachment metadata without letting attachment-free projections erase stored images.

One long-lived app-server supervisor owns JSONL framing, request timeout, stderr tail,
process restart, notification state, and active turn ids. Timeout kills wedged process;
next request restarts and resumes. `thread/compacted` does not complete a turn.
Current app-server versions can return the thread to `idle` without emitting
`turn/completed`; `thread/status/changed: idle` is therefore a terminal fallback.
The supervisor is also the only delivery transport. Presence of Codex desktop IPC
or an open/closed desktop task never reroutes provider prompts and never blocks FIFO.

App-server notifications also maintain a bounded in-memory live projection separate
from Ecky's persisted transcript pages. `agentMessage` deltas, public reasoning-summary
deltas, plan deltas, and tool item lifecycle become transient `working` dialogue
bubbles. Raw reasoning text and command output are not projected. Events carry the
current live projection directly to Dialogue; each delta does not trigger
`thread/turns/list`. Terminal turn state clears live projection, then read-only
reconciliation persists the finished turn and Ecky's cursor-paged transcript remains
authoritative.

FIFO states: `queued`, `sending`, `failed`. Startup recovers stale `sending` to
`queued`. Submit persists enqueue, returns its snapshot, then dispatches outside the
request path. Frontend paints a local queued copy before backend acknowledgement and
reconciles it against the accepted provider user item. Failed head blocks overtaking
and exposes raw provider error/retry/remove. Both `turn/completed` and terminal
`thread/status/changed` advance the next FIFO item.

Queue ownership uses an atomic `queued`→`sending` row claim. No process-wide dispatch
mutex exists: one slow thread cannot block unrelated threads or the recovery scanner.
Enqueue and terminal app-server events wake the supervisor immediately; one-second
polling remains recovery only. Existing bindings dispatch from their stored cwd;
workspace refresh is not placed ahead of every message.

## UI

Dialogue has no provider binding bar, takeover button, picker, id, or release action.
Provider mode reuses normal trail/composer. Ecky messages, authored versions, Codex
messages, and local queued prompts form one timeline; provider snapshot arrival never
replaces Ecky history. Timeline controls provide text search plus `ALL`/`VERSIONS`
filter. It also adds unified pagination, queue, `STEER`, and `STOP` when applicable.
Raw adapter errors appear in Dialogue. Persisted user, generated image, and Ecky sketch tool results render through
the same trail visual path after reload.

Provider final-answer presentation is derived from the raw durable transcript. A
Markdown link targeting an absolute `model.ecky:LINE` becomes a Tactical Midnight
code-reference control only when its path equals the current thread's bound
`ProjectSourceDocument.file`. Activating it loads that bound source into the existing
Code window and selects the exact line. Source-read/path-drift failure stays inline
with the answer and preserves the raw backend detail. Standalone `messageId:` and
`modelId:` evidence lines are omitted from presentation and copy text without mutating
the durable transcript. Provider bootstrap asks adapters to emit useful bound-source
links and keep those internal IDs out of user-facing answers.

Major containers keep `overflow: hidden`; controls use Tactical Midnight tokens and
square borders.

## Failure Rules

- Missing MCP endpoint blocks start/resume/send with raw startup detail.
- Provider MCP uses an integration-private config key so user stdio config cannot
  merge with Ecky's HTTP transport.
- Failed `thread/start` creates no binding and leaves prompt available for retry.
- Background provider read failure keeps locally loaded transcript and does not turn
  a provider reconciliation problem into a thread-loading failure.
- Compaction never clears active turn or dispatches queue.
- Mode switching never deletes durable transcript or binding lineage. Returning to
  Provider displays Ecky history immediately. Delivery resumes the current cursor;
  another client's writer lock leaves the cursor and FIFO prompt intact for delayed
  retry.
- Codex desktop presence or task visibility never becomes a delivery prerequisite.
- Config persistence errors are global `ECKY APP` notifications, never thread bubbles.
