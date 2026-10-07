# Tasks

## 1. Outer red

- [x] 1.1 Add Provider→AGY acceptance for selection and first message routing.
- [x] 1.2 Confirm failure is missing AGY adapter selection.

## 2. Provider-neutral ownership

- [x] 2.1 Migrate bindings to `(ecky_thread_id, provider)` ownership.
- [x] 2.2 Namespace FIFO by provider without losing existing Codex rows.
- [x] 2.3 Add generic Dialogue provider identity and capability presentation.

## 3. Antigravity adapter

- [x] 3.1 Add version probe and NDJSON input/output parser tests.
- [x] 3.2 Add long-lived start/resume process supervisor and raw error capture.
- [x] 3.3 Add local transcript persistence and cursor pages.
- [x] 3.4 Add first-turn bootstrap and workspace Agy MCP config.
- [x] 3.5 Add durable FIFO dispatch, live projection, and exact STOP.
- [x] 3.6 Acquire an Agy writer only for atomically claimed real delivery; never on history read.
- [x] 3.7 Fail stale Agy `sending` rows closed after restart instead of replaying pending work.
- [x] 3.8 Persist per-provider models and pass Agy selection through `--model`.
- [x] 3.9 Replace ignored bare workspace MCP config with an enabled workspace plugin.
- [x] 3.10 Pre-bind provider MCP sessions from exact thread-scoped endpoints.
- [x] 3.11 Bound Agy turns by activity count, repeated activity, and wall time.
- [x] 3.12 Resume a new child when the MCP endpoint changes; avoid handoff duplication on warm turns.
- [x] 3.13 Isolate Agy process groups, stop them on clean Tauri exit, and reconcile durable run leases after crashes.
- [x] 3.14 Copy accepted FIFO attachments into durable provider transcript rows before queue deletion.

## 4. UI

- [x] 4.1 Add AGY provider setting and route.
- [x] 4.2 Render Agy history/live/queue in merged timeline.
- [x] 4.3 Hide unsupported STEER and retain STOP/pending/error states.
- [x] 4.4 Read local durable history without provider activation or takeover UI.
- [x] 4.5 Show only the current active event, retain ordered turn traces, expand interrupted traces, and collapse successful traces before final answers.
- [x] 4.6 Render versions as timeline events and give STOP/STEER primary action weight.
- [x] 4.7 Share bound-source Code references and internal-ID presentation cleanup with Codex.
- [x] 4.8 Keep assistant speech outside expandable `WORKING`; hide accepted `sending` leases.
- [x] 4.9 Preserve raw terminal provider errors across durable refresh and expose retryable FIFO.
- [x] 4.10 Render persisted provider image attachments through the shared timeline after reload.

## 5. Proof

- [x] 5.1 Green Playwright happy and pending/failure states.
- [x] 5.2 Green Rust parser/persistence/supervisor tests.
- [x] 5.3 Green frontend unit tests, production build, and cargo check.
- [x] 5.4 Strict OpenSpec validation.
- [x] 5.5 Browser proof on real route.
- [x] 5.6 Live Agy stream smoke and Codex app-server handshake against installed CLIs.
- [x] 5.7 Green source-reference happy/failure Playwright, prompt tests, strict validation, and installed build.
- [x] 5.8 Green shutdown subtree, crash-recovery lease, prompt-budget, and provider regression verification.

## 6. Unsupported voice input

- [x] Hide Agy voice control and reject standalone NVIDIA STT fallback.
- [x] Prove Agy text composer remains available without recording or STT requests.
