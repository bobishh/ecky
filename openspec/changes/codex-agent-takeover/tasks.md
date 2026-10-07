# Tasks

## 1. Outer red

- [x] 1.1 Replace takeover acceptance with Settings Provider→Codex and automatic first-message ownership.
- [x] 1.2 Confirm red reason is missing `PROVIDER` mode.

## 2. Settings and routing

- [x] 2.1 Add `provider:<adapter>` config interpretation and Provider/Codex controls.
- [x] 2.2 Route provider mode before API/MCP regardless of existing binding.
- [x] 2.3 Remove takeover/discovery/release UI and client surface.

## 3. Owned Codex lifecycle

- [x] 3.1 Add app-server `thread/start`, `thread/name/set`, and cleanup of failed binding.
- [x] 3.2 Lazily ensure binding from first send; writer-conflict recovery now rotates one cursor under section 8.
- [x] 3.3 Resolve deterministic Ecky project cwd and keep one-to-one provider-neutral schema.

## 4. Context continuity

- [x] 4.1 Build start/resume bootstrap from canonical Ecky compacted context.
- [x] 4.2 Project recent Codex context into canonical Ecky summary for API/MCP handoff.
- [x] 4.3 Test mode-switch context and “what are we building?” preservation.

## 5. Runtime controls

- [x] 5.1 Cursor-page transcript by 30-turn opaque cursor.
- [x] 5.2 Persist FIFO and recover stale sending items.
- [x] 5.3 Exact-turn steer/stop; compaction is nonterminal.
- [x] 5.4 Kill/restart on timeout and reconcile fast completion.
- [x] 5.5 Treat idle thread status as terminal when app-server omits turn/completed.
- [x] 5.6 Isolate Streamable HTTP MCP config from user stdio entries.
- [x] 5.7 Materialize and reuse canonical project mirror cwd.
- [x] 5.8 Paint optimistic provider prompts and dispatch persisted queue outside submit request.
- [x] 5.9 Normalize provider transcript order/status and advance FIFO on idle status fallback.
- [x] 5.10 Merge Ecky/provider history with unified older-page loading, version filter, and search.
- [x] 5.11 Keep FIFO delivery on Ecky's app-server regardless of Codex desktop IPC/task visibility.
- [x] 5.12 Replace global dispatch mutex with atomic queue claims and event-driven supervisor wakes.
- [x] 5.13 Project commentary, reasoning summaries, plans, and tool activity as transient live messages.
- [x] 5.14 Present bound `model.ecky:LINE` evidence as exact Code navigation and hide standalone internal IDs.
- [x] 5.15 Persist Codex model choice and apply it to start/resume/turn protocol params.
- [x] 5.16 Type live speech versus activity; keep active `WORKING` expandable and hide accepted sending leases.

## 6. Proof

- [x] 6.1 Green Provider integration Playwright happy/failure/pending cases.
- [x] 6.2 Green frontend routing/config tests.
- [x] 6.3 Green Rust adapter/binding/handoff tests and `cargo check`.
- [x] 6.4 Strict OpenSpec validation and production build.
- [x] 6.5 Browser proof on real route.
- [x] 6.6 Live app-server thread/start, MCP tool discovery, cwd files, and read-only turn proof.
- [x] 6.7 Global config failure renders threadless with raw detail.
- [x] 6.8 Slow-delivery Playwright proves immediate queue paint, stable chronology, and retained versions.
- [x] 6.9 Prove desktop-without-open-task never adds a delivery prerequisite.
- [x] 6.10 Prove live working bubbles render without transcript rereads and atomic claims prevent duplicate delivery.

## 7. Durable Ecky ownership

- [x] 7.1 Persist normalized finished provider messages in Ecky and cursor-page them without provider I/O.
- [x] 7.2 Render local Codex history before any background reconciliation; never activate writer on Dialogue open.
- [x] 7.3 Retain provider binding lineage across supported external cursor replacement.
- [x] 7.4 Preserve durable history and old cursor lineage on active-writer conflict; section 8 supersedes same-cursor retry.
- [x] 7.5 Keep queued prompt durable across active-writer conflict and persist accepted user item under stable turn identity.
- [x] 7.6 Green focused Rust/Playwright proof, strict OpenSpec validation, production app build, and installed app smoke.
- [x] 7.7 Persist provider user attachments, recover Codex image blocks, and render images after history reload.
- [x] 7.8 Persist completed Codex-generated image outputs and render them as assistant timeline images after history reload.
- [x] 7.9 Persist completed Ecky sketch MCP results as concise assistant timeline evidence without replacing canonical source.
- [x] 7.10 Advance thread activity and derive a still-default title when provider history is persisted; backfill older provider-only threads.

## 8. Writer contention recovery

- [x] 8.1 Submit to durable FIFO before attempting to resume the stored writer.
- [x] 8.2 Rotate once to a new Ecky-owned Codex cursor on active-writer conflict, preserving old cursor lineage and normalized timeline.
- [x] 8.3 Keep raw conflict in delivery evidence, show automatic pending state in Dialogue, and retain delayed retry after a second conflict.
- [x] 8.4 Verify focused tests, strict spec, and installed app replacement.

## 9. Exact steering and pending message presentation

- [x] Classify each Jev-enabled Codex `STEER` for its exact active turn; preserve model, reject stale/failed routes before delivery, bind evidence to its persisted message, and avoid FIFO.
- [x] Paint provider send and steer before backend acknowledgement; restore untouched drafts and show raw errors on rejection.
- [x] Reconcile identical pending prompts against newly accepted queue IDs without hiding earlier identical rows.
- [x] Prove routed steering with Rust and pending, accepted, rejected, and repeated-input flows with Playwright.

## 10. Sent image and workspace controls

- [x] Render provider image attachments with the backend-owned pending queue item and submitted draft preview.
- [x] Clear the owning thread's viewport-capture preference and exit Draw mode after acceptance.
- [x] Remove the unused Remember layout checkbox from Dialogue.
- [x] Prove queued image rendering, successful control reset, and failure-state draft retention with Playwright.

## 11. Native Codex voice and runtime catalog

- [x] Reject standalone NVIDIA Speech under Provider/MCP and cancel API capture on mode switch.
- [x] Add failing outer tests for native spoken replies and raw realtime startup failure.
- [x] Add native realtime start/stop with exact owned session identity to the existing Rust supervisor.
- [x] Add WebRTC microphone/playback transport and explicit Codex voice control; retain API STT and hide unsupported Agy/MCP voice.
- [x] Persist completed native speech items into the existing provider timeline.
- [x] Prefer installed desktop runtime for catalog/turns/voice while preserving explicit overrides and PATH fallback.
- [x] Prove voice happy/failure/pending cleanup, current model dropdown, Rust compilation, frontend checks and strict specs.
- [x] Stop pending startup before acknowledgement and route early native errors by exact session identity.
- [x] Require explicit native realtime v3 and prove the AVAS-compatible startup payload.
- [x] Exclude native delegation envelopes from projection, persistence, and legacy history pages while retaining real speech and ordinary messages.
- [ ] Verify live authenticated audio negotiation and actual microphone/playback on the target desktop.
