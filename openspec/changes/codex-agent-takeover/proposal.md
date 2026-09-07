# Proposal: Codex Provider Integration

## Why

Ecky has API and MCP dialogue routes. Neither provides a hosted-agent conversation
whose identity, live turns, queue, and transcript survive app restart. MCP focus
capture also cannot own Codex lifecycle safely.

Codex app-server exposes supported conversation lifecycle, history, event, steer,
and interrupt operations. Ecky can host it as a third provider-integration route
without exposing or importing unrelated Codex conversations.

## What Changes

- Add `PROVIDER` beside `API KEY` and `MCP` in Settings. Provider selection contains
  `CODEX` initially.
- Keep Projects/Ecky thread list as the only user-facing conversation index.
- Lazily create one persisted Codex execution thread on the first provider-mode
  message from an unbound Ecky thread. Retain it as the current cursor until delivery
  must rotate away from a foreign active writer.
- Hide external conversation ids and lifecycle controls. No discovery, takeover,
  release, or foreign-thread picker exists in Dialogue.
- Bootstrap thread/start and thread/resume with Ecky's stable provider prompt,
  embedded MCP endpoint, CAD target identity, and canonical compacted Ecky context.
- Project recent Codex dialogue back into Ecky's canonical compacted thread summary
  so switching among API, MCP, and Provider retains “what are we building?” context.
- Persist finished provider turns in Ecky and page them by opaque local cursor. Keep
  user, generated-image, and completed Ecky sketch-tool result metadata with each normalized message so visuals and native sketch evidence survive reload.
  Keep
  read-only provider backfill, FIFO queue, exact-turn steer, stop,
  timeout recovery, and compaction-safe completion semantics.
- Stream public commentary, readable reasoning summaries, plans, and tool activity as
  transient working bubbles without rereading transcript pages per delta.
- Keep persistence and routing provider-neutral. Codex is first adapter; Claude Code
  can add an adapter without changing Ecky thread ownership or queue schema.

## Product Decisions

- Ecky thread owns provider conversation. External provider history is not an index.
- Binding creation is lazy on first message. Opening an unused Ecky thread creates no
  external junk conversation.
- `connectionType` stores provider choice as `provider:<adapter-id>`; currently
  `provider:codex`.
- Ecky transcript is finished-history authority. External provider threads are
  replaceable execution cursors; Ecky persists normalized turns, lineage, and a
  bounded provider-neutral handoff summary.
- Normal submit during active work queues. `STEER` mutates current turn only. `STOP`
  interrupts current turn only.
- Compaction is progress. Only terminal turn state advances FIFO.

## Out of Scope

- Listing, importing, taking over, releasing, or deleting pre-existing Codex threads.
- Claude Code adapter implementation.
- File/command approval UI.
- Importing unrelated provider-global conversations.

## Proof Plan

- Playwright: Settings Provider→Codex, first-message auto-create, exact routing, no
  takeover UI, raw start failure/retry.
- Playwright: owned-history cursor loading, queue, steer, stop, compaction event, live
  working bubbles and persisted image attachments without transcript loss.
- Rust: start/resume bootstrap, one-to-one binding, handoff projection, atomic queue
  claim/recovery, and app-server delta assembly.
- Frontend unit: provider-mode routing independent of existing binding.
- Strict OpenSpec validation, frontend tests/build, Rust tests, `cargo check`, browser
  proof on real route.
