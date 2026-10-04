# Historical implementation evidence

This records the previous Codex-only increment. Checked items are historical proof,
not evidence that the new global contract is implemented. Current delivery work
is tracked in `tasks.md`.

# Tasks: Experimental Jev Routing

## 1. Specification and existing evidence

- [x] Verify Ecky prompt, queue, model, config, MCP, approval, and Agy boundaries.
- [x] Write proposal, design, normative scenarios, and implementation tasks.
- [x] Audit existing local strict-EDN traces and original command records.
- [x] Separate shared deterministic tool policy from Jev intent/model selection.
- [x] Validate change and synchronized exploration contract with strict OpenSpec.

## 2. Settings: outer BDD first

- [x] Add and run failing Playwright Settings enable/token/save/reload scenario.
- [x] Add missing-token failure and save-pending scenario on a non-user local port.
- [x] Drive typed default-off Rust config and EDN round trip through failing tests.
- [x] Implement experimental checkbox and password input without field-edit network calls.
- [x] Preserve prior durable config on invalid save; redact the typed config's Debug output.
- [x] Audit and prove token redaction across classifier diagnostics and route exports.
- [x] Generate Tauri contracts and pass settings browser scenarios and type checks.

## 3. Intent routing: outer and inner tests

- [x] Add failing queued-turn integration scenarios with a mocked Jev SDK boundary.
- [x] Reuse Lighthouse SDK version/pattern with fixed endpoint/model, bounded context, twelve-second timeout, and no retries.
- [x] Prove malformed-response and timeout behavior; reject authority inferred from a truncated current request.
- [x] Version provisional confidence/margin policy and record ambiguity decisions.
- [ ] Label actual requests and calibrate thresholds/winner margin against replay.
- [x] Test probability validation, Plan/Clarify/mixed contracts, and stale context through unit and production queue fixtures.
- [x] Fix Clarify's self-escalation and prove execution restrictions, not only prompt wording.
- [x] Select catalog prompts and immutable capability policy in Rust before turn start.
- [x] Integrate existing queue claim/retry/cancel without another scheduler or conversation.
- [x] Project pre-dispatch cancellation from Rust and prove UI removal pending/conflict states.
- [x] Prove first reply precedes tools, classifier failure blocks dispatch, and steer cannot widen authority.
- [x] Prove disabled mode uses existing path and makes zero Jev calls.

## 4. Model routing under user ceiling

- [x] Establish sourced, versioned model prices and explicit API-account/OpenAI billing basis; no name-based ranks.
- [x] Add failing available-model/capability/price-ceiling integration scenario.
- [x] Prove verified billing basis, catalog expiry, unavailable ceiling, unknown-model exact identity, and bounded candidate preservation.
- [ ] Extend isolated model tests for componentwise/long-context rates, blank ceiling, and modality rejection as the catalog grows.
- [x] Restrict Jev choices to eligible models; revalidate returned ID in Rust.
- [x] Apply model to next turn on same thread; preserve config and reset sticky overrides.
- [x] Record ceiling/effective model, reason, catalog version, classifier and execution usage separately.

## 5. Deterministic tools and wasted-call audit

- [ ] Label shell pseudo-checks, useful calculations, actual writes, polls, and validations from traces.
- [x] Add failing cached/direct MCP execution restriction tests.
- [x] Prove installed Codex hook/approval boundaries and identify unsupported paths explicitly.
- [ ] Prove Agy's actual command boundary before claiming shared shell enforcement.
- [ ] Add typed rules for proven print-only checks and unchanged-result loops without shell keyword guessing.
- [x] Deduplicate invocation state transitions; preserve required changed-input verification.
- [ ] Reuse Rust budgets/DECIDE for no-progress suspension; no per-call Jev authorization.
- [ ] Future approval UI only: first add pending/allow/deny/cancel/stale-turn BDD; replace unsupported-request handling with scoped decisions. Current increment adds no approval UI.

## 6. Evidence and completion

- [x] Capture Codex routing/tool evidence alongside existing Agy strict-EDN runs.
- [x] Prove successful, failed, and interrupted Codex runs persist trajectories without losing existing dialogue or artifact history.
- [x] Separately reproduce Agy's reported final-answer-plus-rate-limit/authorization failure; identify account/provider route, preserve valid output, and retain the actual redacted terminal diagnostic.
- [x] Prove app-server exit, incomplete/bounded capture, response ordering, malformed starts, and persistence retry retain truthful evidence.
- [ ] Persist pending/active Codex evidence incrementally across a full Ecky process crash; current capture is memory-only until terminal flush.
- [ ] Replay labeled fixtures, one variable per comparison; record false blocks and false write authority.
- [ ] Compare representative live isolated tasks for completion, latency, usage, and total cost.
- [x] Keep optional Jev action assessment disabled and shadow-only until evidence justifies it.
- [x] Synchronize implemented exploration proposal/design/spec/tasks; leave unfinished tasks unchecked.
- [x] Run targeted capture/Agy Rust proof: Codex projection 17, queue/version/persistence retry 1, capture bound 1, Agy adapter 13; UI unchanged in this slice.
- [x] Run relevant Rust tests as compile proof and UI happy plus failure/pending proof.
- [x] Run strict OpenSpec validation and report limitations without claiming deployment.
- [x] Run current workspace Clippy with all targets/features and warnings denied, then compile the desktop application with ordinary `cargo build`.

## Accepted implementation evidence — 2026-09-29

Initial opt-in Codex routing is implemented. Unchecked replay, Agy command
interception, typed waste rules, approval UI, and crash journaling are follow-up
work, not enabled features. Default remains off; no commit or deployment occurred.

- Settings: three Rust JSON/EDN/default/validation tests; ten Playwright scenarios
  on port 5187 including masked-token save/reload, pending save and invalid save;
  generated contracts and frontend checks passed with zero errors/warnings.
- Queue UI: both new outer scenarios failed before implementation; three
  Playwright scenarios then passed for pending removal, conflict, and existing flow.
- SDK: eleven tests passed. Local HTTP fixtures cover request construction,
  malformed decisions/probabilities, timeout, HTTP 429, no retries, exact-token
  redaction, mixed intent, truncation and model eligibility. They are not a live
  TypeSafe account test. Confidence 0.65 and margin 0.15 are provisional.
- Provider contracts: eight policy tests; MCP execution/first-reply evidence gate.
  Reasoning alone does not open the gate; assistant text does; each turn resets it.
- Capture/Agy: Codex projection 17, persistence/version/retry 1, bound 1 and Agy
  adapter 13 passed. Nonempty terminal-error answers retain the raw diagnostic.
- Native boundary: twenty-one app-server tests, three hook-worker tests, production
  queue integration and strict EDN export regression passed. Fresh executable
  build and compiled-worker/actual Rust HTTP callback fixture passed; the fixture
  caught the Axum 0.8 route syntax panic before acceptance.
- Production queue integration covers missing/untrusted-hook failure without
  turn/start, a trusted routed turn and linked version, classifier failure,
  pre-delivery cancellation, stale artifact, disabled zero-Jev path, exact quoted
  native review command, active routed steer becoming FIFO work, and concrete
  native-feature/model restoration after terminal completion.
- The same integration pauses the turn/start reply: ordinary resume cannot
  consume the baseline before active identity exists or during the active turn.
  Resume serialization and the pending marker protect both windows. Settings
  changes cannot revoke the accepted active policy.
- Delivery failures without a provider turn ID use empty identity and no version
  linkage, including an unrelated version created in the same second. Unique
  run_id still identifies the local eval artifact. A final process-exit regression
  first failed on a manufactured process-error ID, then passed with empty identity
  and strict EDN write/read. Interrupted dialogue traces also retain only confirmed
  provider IDs.

Installed Codex probes established individually reviewed startup-hook trust,
interception of shell/apply_patch/view_image/request_user_input and all three goal
handlers, and the write_stdin bypass. Verified native feature suppression blocks
write_stdin at the handler boundary; concrete feature restoration works on the
same durable thread. Null overrides do not reset sticky settings. These probes
establish those tested paths, not universal native/hosted-tool coverage. The actual
callback fixture proves unknown/stale denial, Answer denial, Inspect reply-first
read, and legacy prompt-based passthrough.

Routing retains the configured ceiling when account pricing is unknown or not
comparable. Automatic down-routing uses the sourced, expiring public API catalog
only with confirmed OpenAI API-key billing. Catalog model modalities were checked
independently against official pages. No subscription-cost ordering, measured
savings, threshold calibration or full sandbox claim is made.

No per-tool Jev request, approval UI or crash journal was added. Pending/active
capture remains memory-only until terminal flush. Live classifier/task evaluation
requires an account token and remains unperformed. Known configured-token redaction
preserves typed payloads, original fingerprints, truncation and assistant replies;
it is not arbitrary-secret discovery.

Post-review build correction: the user's desktop rebuild failed with disk exhaustion;
earlier focused tests did not establish that rebuild's success. Production warnings
also exposed a test-only dispatch helper, JSON import and deny fixture compiled
outside tests. They are now confined to test cfg/modules without changing runtime
dispatch. The configured SDK remains connected through `dispatch_queue_for_impl`.
Current `cargo clippy --workspace --all-targets --all-features -- -D warnings`
and ordinary `cargo build` both exited successfully. Formatting and diff checks
passed. Build still reports macOS large `__eh_frame` linker diagnostics; Cargo
reports the existing `block` dependency's future incompatibility. Neither failed
these checks. Inactive generated caches and obsolete test object files were removed
to recover disk space; sources, application data and running user processes stayed
intact. No commit or deployment occurred.
