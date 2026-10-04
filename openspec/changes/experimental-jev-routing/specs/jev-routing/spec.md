## ADDED Requirements

### Requirement: Jev setting is global across provider modes

Ecky SHALL expose one off-by-default Jev classifier setting and one masked TypeSafe
token in common agent settings. It SHALL apply to every application-owned user
request, independent of selected API engine, Codex, Agy, or Ecky-managed MCP agent.
Rust SHALL persist it through canonical EDN. Missing settings decode to disabled;
enabled settings without a token fail without replacing saved configuration.
Credentials SHALL NOT appear in prompts, errors, logs, traces, or exports.

#### Scenario: Switch provider with Jev enabled

- **GIVEN** saved enabled Jev settings and an API connection
- **WHEN** the user switches through Codex, Agy, and managed MCP modes and reloads
- **THEN** the same enabled setting and masked token remain visible
- **AND** no provider switch or Settings edit invokes Jev.

#### Scenario: Invalid configuration preserves saved state

- **GIVEN** a saved configuration and no TypeSafe token
- **WHEN** the user enables Jev and saves
- **THEN** Settings shows the missing-token error
- **AND** the previous durable configuration remains intact.

#### Scenario: Disabled experiment preserves every provider path

- **GIVEN** Jev is disabled
- **WHEN** an API, Codex, Agy, or managed MCP request dispatches
- **THEN** no Jev call occurs
- **AND** the existing provider behavior remains available.

### Requirement: One Rust route serves every application-owned request

Rust SHALL classify a bounded current request before provider execution through a
shared service. Adapters SHALL not implement independent Jev intent rules. Rust
SHALL validate typed probabilities and select Answer, Plan, Clarify, Inspect,
Modify, or answer-plus-action from the same versioned policy. A route SHALL be
accepted at most once per owned request attempt and reused across internal repair
and transport retries. Earlier unfinished work SHALL NOT independently grant
modification authority. User-facing responses SHALL come from the selected
execution provider, not from Jev.

#### Scenario: API engine consumes shared route

- **GIVEN** any configured API engine and globally enabled Jev
- **WHEN** the user asks a question or requests a change
- **THEN** Rust classifies before the engine call and supplies the selected contract
- **AND** the actual engine answers or authors as appropriate
- **AND** Answer, Plan, and Clarify do not create CAD versions merely to respond.

#### Scenario: Agy consumes shared route

- **GIVEN** an owned Agy conversation and globally enabled Jev
- **WHEN** a queued request becomes ready
- **THEN** Rust classifies it before provider delivery
- **AND** Agy receives the selected contract on the same bound conversation
- **AND** Jev use does not require a Codex hook.

#### Scenario: Codex consumes shared route without hook trust

- **GIVEN** an owned Codex thread, globally enabled Jev, and no trusted Ecky hook
- **WHEN** a queued request receives an accepted route
- **THEN** the same bound thread starts its routed turn without `/hooks` setup
- **AND** no hook-trust diagnostic blocks ordinary Jev routing.

#### Scenario: Steering an active routed Codex turn

- **GIVEN** Jev accepted a policy for the active Codex turn
- **WHEN** the user selects `STEER` for that exact turn
- **THEN** Jev classifies the new steer message and Ecky delivers it through `turn/steer` on that exact turn
- **AND** the active turn model stays fixed and the accepted Rust MCP policy applies to that message
- **AND** it does not enqueue the input or broaden tool authority
- **AND** answer-first requires a distinct assistant item after the steer
- **AND** a separate `SEND`/`QUEUE` request receives its own route before a later turn.

#### Scenario: Managed MCP request consumes shared route

- **GIVEN** Ecky owns request delivery to a managed MCP agent
- **WHEN** globally enabled Jev selects a route
- **THEN** Rust binds the selected contract to that request before delivery
- **AND** MCP execution checks reject operations outside the Rust-owned policy
- **AND** the agent cannot widen it by sending a different tool argument.

#### Scenario: Mixed question and change

- **GIVEN** a request asks for both explanation and explicit artifact change
- **WHEN** Jev accepts both judgments
- **THEN** the contract requires a public answer before bounded action
- **AND** a managed MCP agent publishes that answer through `session_answer_save`
  without completing the working request; Rust rejects other MCP tools until
  that observed save succeeds
- **AND** the existing inspect, edit, preview, verify, and version service owns work.

#### Scenario: Blocking ambiguity or truncated current request

- **GIVEN** a strictly blocking missing fact with strong evidence, or truncated current input
- **WHEN** Rust validates the result
- **THEN** it selects Clarify without independent prior-task authority
- **AND** no changed version is manufactured by clarification.

#### Scenario: Continuation with non-blocking uncertainty

- **GIVEN** the user confirms a recent proposal or repeats a requested change, and
  project state or reversible defaults can resolve remaining details
- **WHEN** Jev selects Modify while a separate binary missing-facts judgment has
  moderate confidence for `no`
- **THEN** Rust preserves Modify instead of converting it to Clarify solely from
  the binary confidence score
- **AND** the provider does not repeat a previously answered question.

#### Scenario: Classification appears beside its request

- **GIVEN** Jev is enabled and Rust accepts a route for an owned request
- **WHEN** the route becomes available before or after its user message appears
- **THEN** a separate `classification_result` linked by request and message ID
  projects available Answer, Plan, Clarify, Inspect, and Modify probabilities beneath
  that request, highlighting the selected action without changing the message payload
- **AND** the projection updates without reopening the dialogue and survives reload
- **AND** disabled, pending, failed, or absent routes show no classification label.

#### Scenario: Classifier failure cannot widen authority

- **GIVEN** enabled Jev returns an auth, timeout, or malformed-response failure
- **WHEN** an owned request awaits routing
- **THEN** it fails with the actual credential-redacted diagnostic and explicit retry
- **AND** no unrestricted fallback provider call, synthetic turn ID, or empty version occurs.

#### Scenario: Cancellation or stale context

- **GIVEN** classification is in flight
- **WHEN** cancellation, binding/config change, or artifact revision invalidates the request
- **THEN** the result is discarded before provider delivery
- **AND** existing queue ownership decides whether cancellation or dispatch wins.

### Requirement: Provider-specific model ceilings remain authoritative

Rust SHALL consider only adapter-discovered, capability-compatible model IDs with
verified comparable price and billing basis at or below that adapter's selected
ceiling. Unknown prices, unsupported override, or provider-default selection SHALL
preserve the existing model while intent routing continues. Jev SHALL NOT change
provider/account, save a new model preference, or infer prices from names.

#### Scenario: Comparable cheaper model

- **GIVEN** a configured ceiling and a suitable candidate with comparable lower rates
- **WHEN** Jev selects that validated candidate
- **THEN** the next request uses it through the owning adapter
- **AND** the saved ceiling remains unchanged.

#### Scenario: Unknown Agy or subscription billing

- **GIVEN** the selected provider has no verified comparable model prices
- **WHEN** Jev classifies an intent
- **THEN** the configured Agy or subscription model remains selected
- **AND** route evidence records why automatic model routing was unavailable.

#### Scenario: Invalid model selection

- **GIVEN** Jev names a model outside the eligible set
- **WHEN** Rust validates the route
- **THEN** that model is never dispatched
- **AND** neither action authority nor ceiling increases.

#### Scenario: Provider-default model

- **GIVEN** no reliably resolved explicit ceiling
- **WHEN** Jev classifies a request
- **THEN** intent routing proceeds with provider-default model
- **AND** no invented highest-priced ceiling is assumed.

#### Scenario: Sticky override is restored

- **GIVEN** a routed turn used a cheaper model or native feature restriction
- **WHEN** the next request uses the normal provider default or Jev is disabled
- **THEN** the user's effective prior model and feature settings are restored
- **AND** a prior route cannot leak into the new turn.

#### Scenario: Exact-turn Codex steer is routed freshly

- **GIVEN** global Jev is enabled and a Codex turn remains active
- **WHEN** the user sends a new STEER message for that exact turn
- **THEN** Rust classifies that message and applies its accepted prompt and MCP policy
- **AND** the active turn model remains unchanged
- **AND** Jev failure or stale turn/config/binding/artifact prevents delivery without changing prior policy
- **AND** accepted probabilities bind to the exact persisted steer message and trace the active turn
- **AND** answer-first evidence must come from a distinct assistant item after the STEER.
- **AND** bounded classifier context includes current public live user and assistant items in provider order for equal timestamps, with injected user guidance removed and activity/private reasoning excluded.

### Requirement: Classification is independent from native interception

The Jev setting SHALL NOT install or require Codex hooks, trusted hashes,
provider-native callbacks, approval grants, or native sandbox changes. Rust SHALL
still enforce policy at each Rust-controlled MCP/internal operation. Prompt
contracts SHALL NOT be represented as proven interception of provider-native
shell/file/hosted tools. Additional native enforcement MAY be separately enabled
only with adapter-specific proof and explicit configuration; its absence SHALL NOT
block ordinary Jev intent routing.

#### Scenario: Missing native hook

- **GIVEN** Jev is enabled but an adapter has no native interception boundary
- **WHEN** a valid user request is classified
- **THEN** ordinary Jev routing proceeds without hook registration or trust setup
- **AND** evidence marks provider-native coverage unknown or unsupported.

#### Scenario: Cached/direct MCP mutation call

- **GIVEN** a bound Answer, Plan, Clarify, or Inspect request
- **WHEN** an agent directly invokes a disallowed MCP mutation
- **THEN** Rust rejects it before side effects even if discovery was bypassed.

### Requirement: Shared route evidence remains truthful

Every terminal managed-provider request SHALL retain provider identity, original
user input, classifier judgment/confidence/usage, selected contract, ceiling and
effective model or retention reason, verified boundary coverage, terminal status,
and exact version identities when known in existing strict-EDN and Markdown
artifacts. Unknown usage or cost SHALL stay unknown. Repeated tool-state updates
SHALL count as one invocation. Jev SHALL never authorize tools per call.

The trajectory SHALL carry one local run ID across request admission, Jev input
metadata and decision/error, adapter delivery, observed provider events, and
terminal outcome. It SHALL retain observed event order and actual provider turn
ID only after assignment. Credentials SHALL be redacted. A provider boundary
without event visibility SHALL be marked unknown rather than filled with invented
events.
Managed MCP SHALL durably snapshot the running trajectory before delivery and
after observed tool events, then close it with the actual reply or process-exit
error. An answer-first public answer SHALL remain inside the same open run.
When an agent opens another managed request without saving a final reply, Rust
SHALL close the prior run with that explicit diagnostic. Late results from the
prior request SHALL NOT be attached to the new run.

#### Scenario: Complete observed request trace

- **GIVEN** a managed request routed by Jev
- **WHEN** the provider returns a reply or error
- **THEN** one strict-EDN trajectory links the redacted request, Jev result,
  delivery, observed provider events, final response/error, versions and usage
- **AND** its Markdown report shows the same run ID and terminal result.

#### Scenario: Classifier fails before delivery

- **GIVEN** Jev is enabled and rejects or fails the request
- **WHEN** no provider turn starts
- **THEN** the failed trajectory retains the request and Jev error under one run ID
- **AND** provider turn ID, provider events, and execution usage remain unknown.

#### Scenario: Provider fails before turn identity

- **GIVEN** accepted classification but provider delivery fails before turn ID
- **WHEN** the error artifact is written
- **THEN** it retains a local run ID and raw redacted diagnostic with empty provider turn ID
- **AND** no unrelated version or fabricated tool call is attributed.

#### Scenario: Agy terminal error retains answer

- **GIVEN** Agy produced a useful answer and later returned quota or transport error
- **WHEN** the result is recorded
- **THEN** the answer remains visible with error status
- **AND** the actual terminal diagnostic remains in queue and eval evidence.
