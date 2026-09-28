# Tasks: Exploration Build Cycle

## 1. Reconcile lifecycle semantics

- [x] Remove attempt/candidate/promotion/commit concepts from exploration contracts.
- [x] Reuse normal immutable version IDs, input digests, statuses, render snapshots,
  and verification records.
- [x] Add architecture fitness coverage rejecting parallel exploration authoring
  records and generic commit/finalize operations.

## 2. Outer BDD: every exploratory change is a version

- [x] Add a failing Playwright scenario: start from version A, persist red draft B,
  then green repair C; history shows A/B/C with exact statuses and no extra commit.
- [x] Prove B becomes head while A's last-good viewport may remain visible.
- [x] Prove cycle completion chooses C by ref without creating version D.

## 3. Four-stage reducer

- [x] Add failing Rust tests for `PLAN -> BUILD -> VERIFY -> DECIDE` transitions.
- [x] Implement cycle state, append-only cycle events, budget, evidence refs, and a
  pure reducer.
- [x] Reject BUILD without hypothesis, bounded scope, expected evidence, exact source
  version, or available budget.
- [x] Add COMPLETE, REPLAN, ASK, STOP, and COMPARE decision tests.
- [x] Add ASK suspension and answer-resume tests.
- [x] Make one Rust application service own provider/build/verify/retry transitions;
  remove caller-authored lifecycle facts from normal Tauri and MCP operation.
- [x] Replace generic controller plans with provider-authored typed `BUILD`, `ASK`, or
  `STOP` derived from full context; validate them in Rust before mutation.
- [x] Persist ASK before yielding and resume the same cycle from the recorded answer.

## 4. Prompt contract

- [x] Add prompt tests proving persistence/tool invariants occur once in stable
  system guidance.
- [x] Add dynamic cycle envelope containing goal, acceptance criteria, current
  version/digest/status, last evidence, budget, phase, and required next output.
- [x] Require one typed next step for PLAN; never send a complete executable plan tail.
- [x] Bind repair prompts to exact issue codes, raw diagnostics, and source version.
- [x] Remove stale promote/commit/finalize language from API, MCP, Codex Provider,
  Agy, and capture guidance where normal authoring semantics apply.

## 5. Version-bound build and verification

- [x] Route BUILD through existing append-before-validation services.
- [x] Record provider failures before content change as cycle events without creating
  empty/content-identical versions.
- [x] Bind render and verification evidence to version ID, version input digest,
  render snapshot, and artifact digest.
- [x] Keep deterministic checks authoritative over optional visual evaluation.
- [x] Keep obsolete results out of active viewport without changing version head.

## 6. Latest-pending scheduler

- [x] Add failing actor tests for one running build and latest-pending interactive
  request coalescing.
- [x] Preserve explicit controller BUILD actions without silent coalescing.
- [x] Prove coalescing drops obsolete execution only, never appended versions.
- [x] Surface running and pending counts through existing Ecky state copy.

## 7. Model routing and evals

- [x] Record prompt version, provider, model, reasoning effort, latency, tokens, and
  cost on model-backed cycle events.
- [x] Establish one capable PLAN+BUILD+repair route as baseline; retain cheap intent
  routing and deterministic verification.
- [x] Build representative fixtures for parameter edits, topology changes, constraint
  repair, image-guided reconstruction, and repeated-red recovery.
- [x] Add paired model/effort comparison reports that change one variable and record
  completion, first-build green, repair success, invalid output, unnecessary versions,
  latency, tokens, and cost.
- [x] Keep independent vision results separate and unable to override deterministic
  red evidence.
- [x] Keep production routing on one capable author; model/effort/vision experiments
  remain offline evidence and do not add runtime thresholds or automatic escalation.

## 8. Tauri and MCP boundaries

- [x] Add shared run start/stop intents plus cycle get/active/events/answer projections.
- [x] Keep provider authoring, render, append, verification, retry, and DECIDE behind
  the Rust application service; expose no caller-authored lifecycle transition.
- [x] Keep TS payloads camelCase, Rust fields snake_case, and boundary serde
  translation explicit.
- [x] Return raw errors and exact version/input/snapshot/artifact refs.
- [x] Expose compact cycle packets by default; detailed event/evidence reads remain
  explicit and bounded.

## 9. Workbench projection

- [x] Add failing Playwright happy path for phase copy, immutable version statuses,
  evidence, and completion by existing version ref.
- [x] Show current hypothesis, budget, running/pending build, and ASK through Ecky
  bubble copy without a separate status bar.
- [x] Project immediate message-received activity from an accepted provider turn,
  then yield to exact provider thinking or raw terminal failure.
- [x] Project background parameter render completion onto the still-selected version;
  prove pending-to-success viewport replacement and raw failure with last-good retention.
- [x] Compare ordinary versions; add no candidate UI tier.
- [x] Keep raw failure/evidence detail collapsed under owning version/cycle event.
- [x] Verify Tactical Midnight styling, square borders, bounded overflow, and desktop
  window layout.
- [x] Remove generation, render, verification, and retry state machines from
  Svelte/TypeScript; keep submit/answer/stop and backend-event projection only.

## 10. Restart proof

- [x] Add restart integration coverage for phase, objective, acceptance criteria,
  budget, version refs, evidence refs, route metadata, and pending question.
- [x] Mark unproven in-flight work interrupted at cycle level without mutating
  versions or auto-resuming expensive work.
- [x] Prove existing history/head/success-filter semantics remain unchanged.

## 11. Final proof

- [x] Run `openspec validate exploration-build-cycle --strict`.
- [x] Run targeted Rust reducer, persistence, scheduler, prompt, and command tests.
- [x] Run exploration Playwright happy path plus red-head/pending scenario.
- [x] Run `npm run test:unit`.
- [x] Run `cd src-tauri && cargo check`.

## 12. File-backed LLM trajectory evals

- [x] Add failing Rust integration coverage for strict-EDN run/trajectory files,
  Markdown reports, redaction, bounds, and paired comparison.
- [x] Capture ordered Agy managed-provider assistant/tool/result steps with stable turn,
  route, prompt, timing, and terminal identity.
- [x] Bind each completed turn to immutable versions and verification outcomes created
  during its exact time window.
- [x] Persist `run.edn`, `trajectory.edn`, and `report.md`; create no JSON eval artifact
  or eval database.
- [x] Add deterministic trajectory/outcome graders and same-case one-variable comparison.
- [x] Keep an exporter boundary for later OpenTelemetry projection without making OTEL
  authoritative.
- [x] Run strict OpenSpec validation, targeted Rust proof, and repository CI-equivalent
  checks without staging or committing user work.

## 13. Provider turn intent gate

- [x] Add prompt-based turn contract instructing LLM to categorize intent as `ANSWER`,
  `INSPECT`, `MODIFY`, or `CLARIFY`.
- [x] Remove hardcoded word lists (`ACTION_WORDS`, `INSPECT_WORDS`, `ANSWER_WORDS`) in favor
  of prompt-based turns.
- [x] Put provider intent and capability policy under one Rust authority before Agy
  or Codex turn creation.
- [x] Run `ANSWER`, `INSPECT`, and `CLARIFY` through non-editing modes in both Agy and
  Codex; keep editing mode exclusive to explicit `MODIFY`.
- [x] Filter provider-bound MCP discovery for Agy and Codex by policy and reject
  cached/direct mutation calls.
- [x] Make continuation prompts terminate implicit prior-task authority and state the
  exact unified prompt policy contract.
- [x] Record intent/policy violations in file-backed evals and deduplicate tool-call
  state transitions by stable provider step identity.
- [x] Run strict OpenSpec validation, targeted Rust proof, and CI-equivalent checks
  without staging or committing user work.

## 14. Keep provider policy wrappers private

- [x] Preserve original provider user input separately from transport-only turn
  policy wrappers.
- [x] Project current and legacy wrapped turns as original user text in provider
  dialogue and persisted history.
- [x] Add failure-first Rust projection and persisted-history coverage plus a
  Playwright dialogue contract check.
- [x] Run focused Rust and provider dialogue browser checks; do not stage or commit.
