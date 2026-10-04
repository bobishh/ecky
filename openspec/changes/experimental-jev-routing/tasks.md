# Tasks: Global Experimental Jev Routing

Historical Codex-only work and proof: `implementation-evidence.md`. Checkboxes
below track the revised global contract only. Do not claim global completion from
those historical checks.

## 1. Shared contract and outer red proof

- [x] Add failing Playwright Settings scenarios for one Jev toggle/token across API,
  Codex, Agy, managed MCP modes; include save/reload, pending and missing-token state.
- [ ] Add failing Rust integration fixtures for API, Codex, Agy, and managed MCP
  request delivery through the shared Jev path. Observe actual downstream contract
  and model rather than asserting a standalone helper.
- [x] Add a failing Codex regression: globally enabled Jev with untrusted/missing
  hook dispatches a valid route without hook review.
- [ ] Add failing disabled-mode and classifier-failure scenarios for every dispatch
  family: zero Jev when off, no provider call when on and classification fails.

## 2. Common Rust route

- [x] Extract bounded context, SDK invocation, acceptance, redaction, model
  eligibility, and route evidence into one Rust request-routing service.
- [x] Separate Jev intent classification from optional adapter-native tool
  interception. Remove implicit hook registration/trust gate from Jev activation.
- [x] Keep provisional strong-blocker guard while preserving selected action under
  moderate binary confidence; unit-test ambiguity, truncation, continuation,
  mixed answer-plus-action, and malformed probabilities.
- [ ] Project accepted action probabilities from separate persisted request/message
  result in API, Codex, Agy, and managed MCP dialogue; prove reactive and reload
  behavior plus absent/pending/failed route states.
- [ ] Preserve exact config/binding/request/version identity across classification;
  reject stale/cancelled results before existing atomic delivery boundary.
- [ ] Reuse one accepted route through transport/build retries; never call Jev per
  repair or tool action. Explicit retry may take a fresh snapshot.

## 3. Provider adapters

- [x] API: route all selected API engines before normal response/authoring; actual
  engine provides answers; Answer/Plan/Clarify append no CAD version.
- [x] Codex: consume shared route on same bound thread; remove mandatory startup
  hook setup and verify sticky model/native settings restore.
- [x] Freshly classify each enabled exact-turn Codex STEER, preserve its active model,
  bind its policy/probabilities to its persisted message, and keep it out of FIFO.
- [x] Agy: consume shared route on same bound conversation with existing queue,
  cancellation and failure semantics.
- [x] Managed MCP: bind shared route before Ecky-owned request delivery and prove
  direct/cached MCP execution cannot widen policy.
- [x] Preserve each adapter's model ceiling/default when comparable prices or
  supported overrides are unavailable; reject out-of-set model IDs.

## 4. Global Settings and truthful evidence

- [x] Move existing Jev checkbox/password field to shared agent settings; preserve
  canonical EDN, masked token, validation, and no edit-time network work.
- [x] Capture route and usage in existing strict-EDN/Markdown provider-neutral
  evidence, with exact provider/version identity and unknown cost as unknown.
- [x] Persist one chronological, redacted request -> Jev -> adapter delivery ->
  observed provider events -> terminal outcome trace per managed request, including
  pre-delivery failures, with one run ID and no invented events or turn IDs.
- [x] Mark native shell/file/hosted tool coverage as unknown where interception
  lacks proof; do not claim global native enforcement from prompts or MCP policy.
- [x] Synchronize `exploration-build-cycle` proposal/design/spec/tasks with
  global Jev scope and retain ordinary controller/version ownership.

## 5. Completion proof

- [x] Run focused Rust integration/unit tests with current compile proof.
- [x] Run Playwright happy and failure/pending Settings/request flows on alternate
  port when user dev server is active; typecheck generated contracts.
- [x] Run strict OpenSpec validation for both active changes.
- [x] Report enabled/disabled adapter behavior, unverified live TypeSafe quality,
  model price limits, and remaining native-interception gaps without claiming
  universal sandbox control or measured savings.

Current proof: Settings Playwright 3/3 and Codex pending/failure request Playwright
2/2; Agy serial focused suite plus queued fake-provider disabled/failure/stale/
cancellation delivery; Codex queued untrusted-hook and routed-then-normal model
restoration; API Jev failure/model-policy 2/2; managed prompt and answer-first;
strict-EDN trace 3/3. Rust CI portable tests, 17 selected integrations, formatting,
and Clippy pass. Frontend CI typecheck, unit, component, build, app/docs/landing
e2e pass. API model remains configured because no verified discovered comparable
catalog is supplied. Complete cross-adapter cancellation proof remains open.
Native provider tool coverage is recorded as unknown.
