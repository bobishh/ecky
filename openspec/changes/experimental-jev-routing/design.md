# Design: Global Experimental Jev Routing

## Verified Starting Point

The existing `jev_classifier.rs` SDK client already produces typed judgments,
validates probabilities, bounds context, redacts the token, and reports usage.
Its production integration lives only in `commands/codex_takeover.rs`.
`ConfigPanel.svelte` exposes it only under the Codex provider branch.
Agy dispatch uses `ProviderTurnPolicy::prompt_based()`. API request classification
uses the selected execution engine through `llm::classify_intent`.
Codex Jev dispatch additionally requires a trusted startup hook. These are the
baseline defects this revision replaces, not desired architecture.

## Ownership and Dispatch

```text
application-owned user request
  -> existing Rust request/queue ownership + config/context snapshot
  -> shared Rust Jev route service (when globally enabled)
  -> validated intent + prompt contract + eligible model + route evidence
  -> owning API / Codex / Agy / managed MCP adapter
  -> existing Rust controller, immutable versions, verification, terminal evidence
```

One shared routing implementation owns classification, confidence composition,
model validation, redaction, and the route result. Adapters supply bounded context,
provider identity, configured ceiling, established candidates/billing basis, and
execution capabilities. They consume the accepted result; they do not repeat
classification or invent another provider-specific intent policy.

| Dispatch family | Required integration | Model application |
| --- | --- | --- |
| Every configured API engine | Common Rust user-request routing/controller entry | Per-request engine copy, never saved preference |
| Owned Codex | Existing Rust FIFO dispatch before next turn | Existing same-thread turn model override |
| Owned Agy | Existing Rust FIFO dispatch before next turn | Existing supported model argument on same bound conversation |
| Managed MCP agent request | Rust before Ecky-owned request delivery | Apply only proven adapter override; otherwise retain configured/external model |

No database/config mutex stays locked across Jev network work. Capture request
identity, provider binding, config, ceiling, artifact revision/digest, and queue
ownership before classification. Revalidate before accepted dispatch; cancellation,
stop, binding/config/head/revision changes discard the stale result. Existing Rust
atomic delivery/cancellation boundaries remain authoritative. No second scheduler,
queue, frontend generation state machine, or conversation is added.

Classify once per claimed attempt. Accepted route stays stable through provider
transport retries and controller repair/build retries for that user request.
Explicit retry may classify a fresh snapshot. Every new `STEER` message receives a
fresh route when Jev is enabled, then continues on the exact active Codex turn under
its accepted prompt and MCP policy. The active turn model remains fixed; STEER does
not enter FIFO. Answer-first evidence is scoped to a distinct assistant item created
after the steer. Internal verification, tool calls, and repair prompts do not each invoke
Jev. Project selection, Settings edits, and rendering do not trigger classification.

## Typed Classification and User-Facing Responses

Reuse the existing SDK and bounded request format: current input, recent public
dialogue, summary, artifact facts, attachment modality/explanation, and eligible
models. Treat supplied text as data. Do not send credentials, full files,
transcripts, binary images, or private provider reasoning. Record truncation.
A truncated current request cannot authorize tools from its retained prefix;
select Clarify. Earlier history resolves references, not unfinished-work authority.

Compose action, answer-requested, missing-facts, and model-suitability judgments
in Rust. Preserve the selected typed action when confidence is moderate: TypeSafe
confidence measures distribution concentration, not probability that the selected
choice is true. Override with Clarify only for a truncated current request or a
strong positive judgment that a strictly blocking fact is missing. Recent dialogue
can resolve references and explicit confirmations; unfinished work alone grants no
new modification authority. The provisional blocker threshold remains uncalibrated.

Persist accepted `classification_result` separately from message payload, keyed by
Ecky thread/provider/request ID and bound to the user message ID when known. The UI
projects the action probability distribution as badges under that message or pending
queue entry, highlighting the selected action and refreshing on acceptance and
reload. No provisional label for pending or failed classification.

| Contract | Requested behavior | Rust-controlled tool policy |
| --- | --- | --- |
| Answer | Answer current input from supplied context | No authoring |
| Plan | Explain proposed work, no built-in planning-mode transition | No authoring |
| Clarify | Ask one focused question, no self-escalation | No authoring |
| Inspect | Reply then bounded read-only inspection | Reads only |
| Modify | Reply then requested inspect/edit/preview/verify | Scoped authoring |
| Answer + Inspect/Modify | Address question before action | Accepted action policy |

Jev generates no user-facing answer. API mode must send the selected contract to
its actual execution provider for response or authoring. Do not return an empty
`IntentDecision.response`, fabricate a canned answer, run the old LLM intent
classifier after Jev, or route Answer/Plan/Clarify through CAD generation that
creates a version. Keep normal API response contracts and usage projection.
Owned provider sessions receive the same selected contract. Managed MCP delivery
carries it as Rust-owned bound-request policy and guidance; external agents cannot
self-select a different policy through tool arguments.

Classifier timeout/authentication/malformed output/missing token fails the current
attempt with the actual credential-redacted diagnostic. No unrestricted fallback
or reconnect loop for classifier errors. Existing typed retry ownership separates
classifier failure from provider transport failure. Failure before dispatch creates
no provider turn identity or synthetic version.

## Provider-Local Model Ceilings

A model ceiling belongs to the selected adapter: API engine `model`,
`providerModels.codex`, `providerModels.agy`, or managed agent model configuration.
A Jev result cannot replace the selected provider/account/endpoint. Discovery IDs,
capability suitability, prices, currency, units, billing basis, and catalog freshness
must be established before offering cheaper candidates. No name-based ranking.

Unknown or incomparable prices and unsupported model overrides retain the current
ceiling/default while intent classification still runs. Agy and subscription-backed
Codex must not inherit OpenAI API price assumptions. API compatibility endpoints
must not inherit OpenAI billing merely because their wire format is compatible.
Blank ceiling preserves provider default. An unavailable explicit ceiling produces
the existing actionable configuration diagnostic, not an invented replacement.

Retain the existing sourced price catalog only for matching confirmed billing.
Record catalog version/expiry and unknown reasons. Applicable input, output, cached,
and long-context rates must not exceed the ceiling. Validate returned model ID
against the exact eligible set again. Actual task cost is not inferred from token
rates. Use a per-request model override and restore defaults on later disabled
requests; never save a routed model over user preference.

## Classification and Native Enforcement Are Independent

`jevClassifier.enabled` means classify application-owned requests for every
provider. It must neither register Codex startup hooks nor require `/hooks`,
hook trust, a native interception callback, or a hook setup terminal to proceed.
A valid accepted route dispatches when the optional native boundary is absent.
Ordinary Jev must not disable whole classes of provider-native tools as a substitute
for the missing hook; preserve adapter defaults except proven contract-specific
restrictions. Restore any sticky restrictions from the superseded routed path.

Rust-controlled MCP and internal operations still enforce selected capability
policy at discovery and execution, including cached/direct tool calls and required
first-reply evidence. A prompt is guidance for provider-local tools, not proof of
shell/file interception. Route evidence must distinguish enforced boundaries from
unsupported/unknown native coverage. No full sandbox guarantee is made.

If retained, additional native enforcement needs separate explicit configuration,
exact adapter-specific interception proof, and its own availability/trust gate.
Only a request explicitly selecting that enforcement may fail for its absence.
Do not enable it through the global classifier flag or trust unrelated hooks.
Codex probe evidence remains in `codex-boundary-notes.md`; it does not impose
requirements on Agy, API engines, or managed MCP agents. No new native-enforcement
settings UI is required by this change.

## Global Settings and Persistence

Expose the existing `jevClassifier: { enabled, apiKey }` once in common agent
settings, independently of selected connection/provider. Keep default off,
password input, canonical `:jev-classifier` EDN, and existing `save_config` path.
Changing provider shows the same value; it does not create per-provider copies.
Invalid enabled/blank-token save retains prior config. Pending and actual error
states remain visible. No classification occurs during Settings interaction.

Rust field names stay snake_case; all boundary structs use camelCase serde.
Token is redacted in Debug/errors/logs/prompts/evidence/export. Preserve existing
local persistence behavior; masking is not encryption.

## Evidence and Proof

Use existing strict-EDN runs and Markdown reports, not a new eval database.
Each request gets one local run ID before Jev. Its chronological trajectory
contains the redacted original request, bounded classifier input metadata and
truncation flags, Jev judgment/probabilities/usage/error, accepted contract,
adapter/model decision, actual delivery attempt and provider turn ID if assigned,
provider output/tool/result events when observed, terminal reply/error, version
refs and execution usage. Record provider/adapter identity, route/prompt/threshold/
catalog versions, selection/retention reason and verified boundary coverage.
Persist a failed run even when Jev or delivery fails before provider turn creation.
For managed MCP, persist a running snapshot before delivery and after each observed
tool start/result; close pending runs with the process-exit error. For mixed
answer-plus-action turns, expose only `session_answer_save` until its successful
public message appears in the same run. Keep the user request working and trace
open for the subsequent action and final `session_reply_save`.
Unknown price/usage stays unknown. No synthetic provider turn ID or unrelated
same-second version linkage after pre-dispatch failure. Redact credentials and
sensitive text in every event payload; full trace means full observed sequence,
not unredacted secrets or invented provider events.

Outer tests cover global Settings in API, Codex, Agy, and managed MCP modes with
save/reload plus missing-token/pending behavior. Rust adapter integrations must
observe actual classifier input and actual downstream contract/model behavior,
not just test a helper in isolation. Cover disabled zero-Jev, failure no execution,
stale/cancelled classification, retries reusing a route, and same history/binding.
Codex acceptance must prove valid Jev dispatch with no trusted hook.
Agy/API/MCP acceptance must prove the global flag is consumed.

Historical checked tasks are in `implementation-evidence.md`. Current tasks in
`tasks.md` remain unchecked until current behavior is implemented and proven.
Retain replay/calibration/live-cost/native-interception/crash-journal limitations.

## References

- TypeSafe SDK and existing `jev_classifier.rs` implementation.
- `../exploration-build-cycle/`: shared Rust lifecycle and immutable versions.
- `implementation-evidence.md`, `codex-boundary-notes.md`, `trace-audit.md`: historical evidence only.
