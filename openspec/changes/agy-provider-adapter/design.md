# Design: Antigravity provider adapter

## Provider contract

Provider mode resolves `provider:<id>` into an adapter capability set. Shared Ecky
ownership, FIFO, context handoff, optimistic messages, and timeline presentation stay
provider-neutral. Transport remains adapter-specific:

- Codex: app-server protocol, Ecky-authoritative cursor history with read-only
  provider backfill, replaceable external execution cursor, steer + stop.
- Agy: one bidirectional NDJSON child per active conversation, Ecky-authoritative
  cursor history, stop, no steer.

Dialogue uses `providerId`, `providerLabel`, external conversation id, and
capabilities. It never exposes a provider-global conversation index.

## Persistence and migration

`agent_thread_bindings` uses `(ecky_thread_id, provider)` as primary key and keeps
`(provider, external_thread_id)` unique. `agent_prompt_queue` includes `provider`,
references the composite binding, and orders FIFO inside that pair. Existing Codex
rows migrate as `provider = 'codex'`.

`agent_provider_messages` stores normalized Agy user/assistant transcript entries and
serialized user attachment metadata.
Rows are keyed by stable id and indexed by `(ecky_thread_id, provider, created_at,
id)`. Cursor pages use an opaque encoding of the final sort key. Full Agy global
history is never imported.

The claimed FIFO row copies its prepared attachments into the accepted user transcript
before queue deletion. Dialogue projects persisted image paths/data into the shared
timeline visual renderer, so switching providers or reopening the thread loses no image.

## Agy lifecycle

Minimum supported CLI version: `1.1.15`. Every start checks `agy --version`; an old
or missing executable returns the raw actionable error.

The adapter starts in the canonical project mirror:

```text
agy --input-format stream-json --output-format stream-json --print-timeout 30m
```

Configured model selection adds `--model <stored-id>`; blank selection leaves the flag
absent. Resume adds `--conversation <stored-id>`. `--conversation` is an active resume,
not a passive subscription, so opening Dialogue never spawns it. A prompt writes one NDJSON `user` event.
The first outbound content prepends a versioned Ecky bootstrap containing thread id,
title, cwd, canonical handoff, exact MCP endpoint, workspace tool-guide path, and
inspect→validate→preview→verify rules. Antigravity does not discover a bare
workspace `.agents/mcp_config.json`; Ecky therefore materializes a workspace plugin
under `.agents/plugins/ecky-provider/` with `plugin.json`, `mcp_config.json`, and
`rules/AGENTS.md`. The plugin uses Antigravity's `serverUrl` schema and the
`ecky_mcp` name, intentionally overriding a same-named stale global entry through
workspace priority. Ecky never mutates user-global Agy configuration.

The plugin endpoint carries `providerThreadId=<exact Ecky thread>`. MCP initialize
pre-binds that live session before the first tool call. Provider prompts and tool docs
state that `thread_borrow` is only for an intentional switch to a different target;
the assigned provider thread MUST NOT borrow itself. `workspace_overview` therefore
works as the first call. Re-created MCP sessions recover the same query-bound target.

The adapter waits only for `init` before persisting a new binding. Remaining output
streams asynchronously. One terminal `result` completes each turn and wakes FIFO.
Process exit without a terminal result becomes a raw retryable runtime error. A
subsequent turn resumes the stored conversation in a new process.

An accepted `sending` row is not safe to replay after Ecky restarts: another orphaned
Agy process or provider-side pending turn may still own the work. Startup therefore
fails stale Agy delivery closed with explicit recovery detail. Each accepted Agy row
stores a run id, PID, isolated process-group id, executable, and external conversation
id. Clean Tauri exit explicitly kills every owned group before `App::run` reaches its
`process::exit` path. Crash recovery verifies the recorded PID, command shape, and PGID
before killing a matching orphan group. Legacy rows without a lease use only an exact
orphan `agy --conversation <owned-id>` plus stream-json signature; a non-matching PID
is left untouched and the queue error says the agent may still be running. Codex stale
rows retain their adapter-specific recovery path.

## Stream projection

- `step_type=agent_response` + `text_delta`: live ordinary assistant response.
- tool step: bounded `USING TOOL · <name>` working bubble; raw stdout omitted.
- other public step types: bounded `WORKING · <type>` bubble.
- terminal success: persist user and final assistant response, clear live projection.
- error/canceled/interrupted: preserve prior transcript and raw terminal error/status.

Live turn state stays bounded to 256 public events and 16,384 characters per event. Persistent
pages contain 30 messages. The request path returns after durable enqueue; queue
delivery runs in the supervisor.

Agy turns have no activity-count, repeated-action, or elapsed-time kill guard. They
continue until provider result, explicit user `STOP`, process exit, or transport error.
A changed MCP endpoint invalidates the warm child; the next turn resumes the same
conversation in a new process using the new workspace plugin config.

The shared frontend classifies provider events explicitly as `assistant` or `activity`.
Only activity enters the expandable `WORKING` plaque; live assistant speech remains a
normal chronological reply. Terminal success moves activity into bounded turn history,
collapsed by default, and leaves the persisted final answer visible after it.
Interrupted and failed traces open by default while preserving interleaved speech and
activity order. Starting a queued turn does not delete earlier terminal traces.
Version rows stay independent, full-width timeline events so provider-authored model
changes cannot read as chat prose.

## Stop and steer

Agy stream-json accepts only text input blocks. Its interactive CLI `/voice` and
`mic-serve` features do not supply an audio input contract for Ecky's headless
adapter. Dialogue therefore omits `VOICE`; standalone NVIDIA STT is rejected in Agy
provider mode before remote I/O.

Agy stream input documents only `user` events. Therefore Agy does not claim steer.
Dialogue omits `STEER` when adapter capabilities say false. Every Agy CLI starts in an
isolated process group. `STOP` sends SIGINT to that exact group. If no terminal result
arrives within a bounded grace period, the whole group is killed. Pending FIFO remains.
Next turn resumes the current conversation id.

## Mode switching and compaction

Switching API/MCP/Codex/Agy never deletes bindings. Each provider snapshot updates a
bounded canonical Ecky summary. Starting or process-resuming another provider receives
that summary. Warm continuation turns send only the new user message and a short
pre-bound-session reminder; they do not resend the canonical handoff. Agy result
boundaries, not compaction events, determine turn completion; long-lived stream reading
continues through provider compaction phases.

## Failure behavior

- Old CLI: raw version requirement; prompt remains retryable.
- MCP file write/start/init failure: no Agy binding; raw error visible.
- Turn/result error: failed queue head; prior transcript retained.
- Process hang: timeout/stop interrupts child; UI remains responsive.
- Ecky clean exit during Agy delivery: owned process group stops before Tauri exits.
- Ecky crash during Agy delivery: verified orphan group is reaped; stale row fails closed; no automatic conversation replay.
- Unsupported steer: control absent, backend rejects direct calls.
