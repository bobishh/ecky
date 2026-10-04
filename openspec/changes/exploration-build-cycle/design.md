# Design: Exploration Build Cycle

## Context

Ecky version history is already append-only and lossless. A version is the exact
authoring snapshot; its mutable outcome metadata may move through `pending`,
`working`, `success`, `error`, or `discarded`. Rendering success does not
decide whether a version exists.

The old exploration design introduced immutable attempts, promoted candidates,
and explicit candidate commits. That model duplicated version identity and
contradicted append-before-validation. The replacement keeps one authoring truth
and adds a small orchestration state machine around it.

## Goals / Non-Goals

Goals:

- make exploration bounded, evidence-driven, replayable, and restart-safe;
- choose one next revision instead of executing a stale complete plan;
- retain every changed draft as an ordinary immutable version;
- attach exact failures and verification evidence to the owning version;
- keep last-good viewport projection separate from latest-version head;
- make prompt and model decisions measurable.

Non-goals:

- create attempt/candidate/version tiers;
- gate persistence on render or verification success;
- add manual promotion, commit, or finalize operations;
- ask an LLM to own lifecycle transitions;
- assume expensive planning plus cheap implementation is inherently better;
- replace current source persistence, watcher, render, or verification services.

## Authoritative Records

| Record | Meaning | Mutability | Authority |
| --- | --- | --- | --- |
| Version | Exact persisted authoring snapshot | Content and identity immutable; outcome evidence attaches | Version history |
| Exploration cycle | Bounded objective over versions | Phase, budget, pending question, referenced versions | Cycle controller |
| Cycle event | Plan, phase transition, question, answer, or decision | Immutable append event | Cycle event log |
| Render snapshot | Geometry produced from one version input digest | Immutable | Render service |
| Verification record | Evidence for one version/artifact identity | Immutable | Verification service |

No separate attempt or candidate record exists. If a model/provider call fails
before authoring content changes, the failure is a cycle event. If changed content
is persisted, that content is a version even when parsing, rendering, or verification
fails.

### Exploration cycle

Minimum fields:

```text
cycleId
threadId
baseVersionId
objective
acceptanceCriteria
hardConstraints
softPreferences
phase
budget
budgetUsed
currentVersionId?
chosenVersionId?
pendingQuestion?
lastEvidenceRef?
promptVersion
createdAt
updatedAt
```

`baseVersionId` is exact and immutable. `currentVersionId` follows work performed by
this cycle, not an ambiguous generic latest lookup. `chosenVersionId` is a cycle
result pointer only; choosing it does not promote, copy, or mutate the version.

### Cycle event

Minimum fields:

```text
eventId
cycleId
phase
sourceVersionId?
resultVersionId?
hypothesis?
expectedEvidence?
evidenceRefs
decision?
modelRoute?
rawError?
createdAt
```

Model route records provider/model/effort/prompt version for evaluation and audit.
It never participates in version identity or artifact cache identity.

## Adaptive Four-Stage Loop

The four stages are controller phases, not four persisted authoring object types.
Only one phase is active at a time.

### 1. PLAN

Inspect exact current version, current artifact/verification evidence, user goal,
acceptance criteria, constraints, and remaining budget. Produce one bounded next
step:

```text
action: BUILD | ASK | STOP
hypothesis
changeScope
expectedEvidence
sourceVersionId
```

The plan may preview likely later work for user explanation, but only this next step
has execution authority. The reducer rejects `BUILD` without exact source identity,
hypothesis, expected evidence, or budget.

### 2. BUILD

Apply one bounded source/parameter change. Existing authoring services own the
pipeline:

```text
inspect exact source
  -> append changed snapshot as immutable version
  -> validate / preview / render
  -> attach outcome and raw evidence to that version
```

Identical content is a no-op and creates no duplicate. A changed invalid draft still
creates a version and becomes head. The controller never waits for success before
persisting it.

### 3. VERIFY

Verification is layered:

1. parse, type, parameter, constraint, and backend capability checks;
2. render/artifact identity checks;
3. deterministic structural and authored `(verify ...)` clauses;
4. optional screenshot/vision evaluation for visual or mechanical intent not
   established by deterministic evidence.

Every record names the exact version ID, `versionInputDigest`, render snapshot, and
artifact digest it evaluated. Verification attaches evidence; it does not create a
version or decide history membership.

### 4. DECIDE

The deterministic reducer chooses among:

- `COMPLETE`: acceptance criteria satisfied; store chosen version ref;
- `REPLAN`: repairable red evidence or a useful next hypothesis exists;
- `ASK`: user choice materially affects geometry or acceptance;
- `STOP`: budget exhausted, user stopped, or no justified next change exists;
- `COMPARE`: expose referenced versions without promoting either.

`REPLAN` returns to `PLAN` with exact new evidence. `ASK` persists question plus the
decision it blocks. `COMPLETE`, `STOP`, and `COMPARE` never alter version status.
The user may explicitly choose a red or artifact-less version; cycle selection and
technical verification status remain separate facts.

## State Transitions

```text
idle -> planning
planning -> building | awaiting_input | stopped
building -> verifying | deciding
verifying -> deciding
deciding -> planning | awaiting_input | completed | stopped
awaiting_input -> planning | stopped
completed -> stopped
```

The reducer validates identity, budget, evidence, and phase. Model output proposes
content or a typed next step; it never mutates cycle state, version history, or
viewport projection directly.

## Controller Authority

The Rust application service is the sole owner of cycle and generation-run
lifecycle. It performs or authorizes provider calls, immutable draft appends,
render, deterministic verification, retry/replan, queue arbitration, and terminal
decisions. Tauri and MCP are adapters over that same service.

Frontend code may:

- submit an objective or explicit user action;
- provide requested visual evidence captured from the viewport;
- answer a persisted ASK or stop a cycle;
- subscribe to compact progress events and project backend packets into stores.

Frontend code may not:

- maintain a generation-attempt or verification retry loop;
- decide whether red evidence retries, completes, or becomes accepted;
- manufacture `BUILD started`, `version appended`, or `verification recorded`
  lifecycle facts;
- arbitrate latest-wins work or infer completion from component-local state.

An application-window reload can lose ephemeral UI projection without changing
the durable cycle. Process restart marks unproven in-flight backend work
interrupted and never reconstructs authority from Svelte stores.

Persisted parameter applies may return while their Rust render is still working.
The `parameterVersionRendered` / `parameterVersionFailed` history events therefore
refresh the exact selected version's outcome as well as its timeline row. Successful
completion loads that version's persisted runtime; failure exposes its raw diagnostic
while retaining the last-good viewport. Selection identity is checked before and after
the detail read so a stale completion cannot switch the user's thread or version.
This is UI projection only and performs no render, retry, or version append.

## Prompt Contract

### Current split

- API mode uses a stable design system prompt and a dynamic contextual user payload.
  The model emits one `DesignOutput`; the application owns render, structural verify,
  optional vision verify, and bounded repair.
- MCP/provider mode uses shared authoring guidance. Bound-file edits trigger watcher
  append/validate/preview; the agent then calls verification. No commit exists.

### Required delta

Keep stable system instructions lean and cacheable. State each invariant once:

- every changed draft is already a version before validation;
- build/verify attach status and evidence to that version;
- no promote, commit, or finalize action exists;
- stop only on acceptance, required user input, explicit stop, or budget exhaustion.

Put changing cycle data in the dynamic context, after the stable prefix:

```text
GOAL
ACCEPTANCE CRITERIA
HARD CONSTRAINTS
CURRENT VERSION ID + INPUT DIGEST + STATUS
LAST VERIFICATION EVIDENCE
REMAINING BUDGET
CURRENT PHASE
REQUIRED NEXT OUTPUT
```

For `PLAN`, require one typed next step, not a complete executable program. For
`BUILD`, provide the accepted next step, exact current source/version identity, and
relevant diagnostics. For repair, include exact red issue codes/raw errors and ask
for the smallest change that addresses them. `VERIFY` remains deterministic first;
vision receives original intent, acceptance criteria, deterministic evidence, and
render images. `DECIDE` is reducer-owned when evidence is conclusive.

PLAN is not a controller-written placeholder. The authoring turn derives its concrete
hypothesis, bounded scope, expected evidence, or ASK/STOP decision from the complete
assembled context. Rust validates that typed action against current version and budget
before accepting any accompanying BUILD output. Planning metadata is stored in cycle
state/events and never inside the immutable version payload.

PLAN is not a version, message class, or separate product entity. It is the typed action
portion of the same authoring turn that may also contain BUILD source. After validation,
Rust copies that exact action into the append-only cycle event for audit/restart. Only a
changed CAD source snapshot enters immutable version history.

PLAN and BUILD are conceptual phases, not mandatory separate provider requests. The
MVP keeps them in one authoring turn when one model can safely inspect and edit. A
separate planning call is allowed only when evals show a quality gain that exceeds
handoff cost and plan staleness.

## Model Policy

### MVP route

- Cheap/deterministic router: question vs design, simple capability routing, and
  context shaping.
- One capable authoring model: PLAN + BUILD + red-to-green repair. Same model/context
  retains geometric intent and exact diagnostics.
- Deterministic backend: parse, type, constraint, artifact, structural, and authored
  verification. No LLM substitutes for these gates.
- Optional vision model: visual/reference comparison only. It may be independently
  selected when a measured vision-capable route meets the quality bar.

Do not default to a strong planner plus weak implementer. CAD implementation is where
language precision, topology reasoning, and diagnostic repair are required; weakening
that stage loses the plan during translation. A cheap router plus strong author is a
safer first split. A separate strong planner is justified only for genuinely broad,
ambiguous work and still hands off one bounded next step.

### Runtime route

Production uses the configured capable author for PLAN, BUILD, and repair. Runtime
does not contain automatic model escalation, reasoning thresholds, or planner/author
model splitting. Alternate model, effort, and vision routes are offline experiments;
adopting one requires a later explicit product decision backed by recorded results.

## Provider turn intent and capability policy

Every provider message is routed before the tool-capable turn starts:

```text
ANSWER   explain, report status, or respond from supplied conversation context
INSPECT  gather bounded read-only evidence, then answer
MODIFY   perform one explicitly requested bounded authoring change and verify it
CLARIFY  ask one question because write intent or desired outcome is ambiguous
```

Default provider turns are prompt-based: the prompt contract presents the unified turn policy
(`ANSWER`, `INSPECT`, `MODIFY`, `CLARIFY`) and the LLM itself categorizes the user's
intent. Hardcoded word dictionaries (`ACTION_WORDS`, `INSPECT_WORDS`, `ANSWER_WORDS`)
are an anti-pattern and are prohibited as they fail to generalize across phrasing,
context, and languages.

The revised `experimental-jev-routing` contract adds one explicitly enabled global
Rust route for every application-owned API, Codex, Agy, and managed MCP request.
Jev classifies bounded context; Rust selects the prompt contract and a model only
when an adapter has a verified comparable ceiling. Existing provider ownership
executes the result. Codex hooks are optional native enforcement, never a Jev
prerequisite. The previous Codex-only implementation remains historical baseline;
current cross-adapter tasks record global implementation proof. The route remains off by default; confidence
blocker thresholds and live-account quality remain uncalibrated. The selected
typed action is preserved for moderate scores; only a strong blocking-fact
judgment or truncated current input forces Clarify. Accepted classification is
projected with action probabilities through a separate result keyed to the request and message, never by
expanding the message payload. It adds no exploration
controller, version lifecycle, or per-call Jev authorization.
Each Codex `STEER` is freshly classified by global Jev when enabled. Accepted intent
updates the Rust MCP policy for that exact active turn and wraps only that steer
message; the turn's model remains fixed. Classification failure or stale
turn/config/binding/artifact prevents delivery and leaves the prior policy unchanged.
A normal
`SEND`/`QUEUE` enters Rust FIFO and receives a separate route when dispatched.

The unified prompt contract instructs the model:
- `ANSWER`: explain, report status, or respond directly from conversation context without calling tools or editing files.
- `INSPECT`: gather bounded read-only evidence using advertised inspection tools, then answer.
- `MODIFY`: perform one explicitly requested bounded authoring change, inspect, edit, preview, and verify.
- `CLARIFY`: ask one concise question without tools when intent or parameters are ambiguous.

Where programmatic policy is enforced (e.g. in test suites or explicit read-only evaluations),
Rust enforces tool allowlists and non-editing mode. In standard interactive turns, the
unified prompt contract governs turn categorization. Continuation prompts explicitly
terminate implicit carry-over: the current user message alone grants turn authority.
A status question does not resume an incomplete repair. Unstarted prompts remain durable,
but no older prompt may silently widen the new turn's capability policy.

Turn evaluation records classified intent, effective policy, unique tool-call count,
policy violations, and versions created. Streaming `active`/`done` updates sharing
one provider step identity are one tool call, not repeated calls.

### Evaluation

Route decisions ship only after replaying representative cycle fixtures. Compare:

- acceptance-criteria completion rate;
- first-build green rate;
- red-to-green repair rate;
- invalid-language and unsupported-op rate;
- unnecessary version count;
- total tokens, latency, and cost;
- user questions and unjustified stops.

Test one variable at a time: prompt version, model, or reasoning effort. Record route
metadata in cycle events. Never encode provider-specific quality claims as domain
rules.

## File-backed LLM evaluation artifacts

Provider activity shown in the workbench is a live projection, not durable eval
evidence. Terminal Agy and queued Codex managed-provider turns write one bounded artifact
directory below
`app_data_dir/evals/runs/<run-id>/`:

```text
run.edn          strict data-only run metadata, outcome, route, and version refs
trajectory.edn   ordered strict-EDN event vector with safe tool inputs/results
report.md        deterministic human-readable scores and diagnostics
```

No JSON eval artifact or eval database is introduced. Provider JSON may remain an
upstream transport protocol, but it is decoded before persistence. Secret-shaped
keys are redacted recursively. Oversized values are clipped with their SHA-256
digest retained. The eval writer never stores provider credentials or unbounded
terminal output.

The artifact schema and writer are provider-neutral. Agy and Codex ingestion bind
provider conversation and turn identity to the Ecky thread, original queued user
prompt, configured model/effort when known, start/end timestamps, terminal state,
ordered provider steps, and immutable versions appended during that interval.
Deterministic scoring derives unique tool-call count, repeated-tool count, duration,
first-build-green, completion, red-to-green repair, and unnecessary red versions.
Paired comparison changes one route variable and emits Markdown. An optional
telemetry exporter may later project the same bounded metadata to OpenTelemetry;
telemetry is never the authoring or eval source of truth.

Codex captures success, error, interruption, and app-server subprocess exit through
the existing Rust queue supervisor. Tool item IDs bind started/completed transitions
to one invocation; user and assistant items do not become tools. Missing identities,
unsupported items, orphan completions, and capture limits are explicit incomplete
evidence. Capture reserves space for both that diagnostic and terminal status.
An active-writer delivery conflict before provider turn identity is recorded as a
failed attempt with the raw diagnostic. Rust creates one new Ecky-owned execution
cursor for the queued request, preserves old cursor lineage and normalized history,
and delivers the same FIFO head. A second conflict for that queue ID waits for
bounded retry rather than creating unbounded threads. Dialogue projects transient
contention as waiting; terminal provider failures still expose their raw body.
App-server-reported model/effort override configured route metadata when present;
an absent configured or reported value remains unknown.

The supervisor acknowledges a Codex run only after version linkage and file writes
succeed. Failed writes or version queries retry the same run ID. Saved files survive
restart; pending and active Codex capture remains in memory, so an abrupt full Ecky
process exit can lose unfinished or unflushed evidence. This change does not claim
an incremental durable capture journal.

Agy preserves a nonempty terminal answer even when the provider reports an error,
including a recovered failed result. The dialogue status and queue error remain
truthful; retained answer content never converts a provider failure to success.

## Scheduling And Publication

One cycle runs at most one expensive build at a time.

- Explicit controller BUILD steps are never silently replaced.
- Repeated unstarted interactive rebuilds for the same working target coalesce to the
  newest exact input.
- If an obsolete running render completes, its evidence remains attached to its
  version but it cannot replace the active viewport projection.
- Only the newest eligible non-obsolete render may publish the active viewport.
- Version head still follows append order and may be red while viewport shows an older
  successful snapshot.

Coalescing execution work does not delete versions already appended by source/draft
persistence.

## Persistence And Recovery

Backend services own cycle events and version mutations. Frontend, watchers, and
agents use Tauri or MCP boundaries; none write SQLite directly.

Persistence order:

```text
persist PLAN event
  -> reserve budget
  -> persist changed draft through version append service
  -> run validation/render
  -> attach version outcome
  -> attach verification evidence
  -> persist DECIDE event
```

Restart marks an unproven in-flight build interrupted at cycle level. Existing
version and evidence records remain unchanged. Recovery restores pending questions
and refs but never automatically resumes model calls or expensive render work.

## UI Projection

No new global status bar.

- Primary history shows every immutable version and its status.
- Successful/printable filtering remains a projection.
- Ecky bubble copy shows cycle phase, current hypothesis, budget, pending question,
  and running/pending build state.
- Comparison selects ordinary version refs; no candidate UI tier exists.
- Raw errors and verification evidence expand under the owning version/cycle event.
- Last good viewport may remain visible while latest head is red or pending.

Major layout containers keep `overflow: hidden`; controls follow Tactical Midnight
styling and square borders.

## Boundary Contracts

Frontend and MCP-facing JSON use camelCase. Rust fields use snake_case. Every Rust
boundary struct uses `#[serde(rename_all = "camelCase")]` unless a protocol-fixed
JSON-RPC shape requires otherwise.

Minimum caller intents and projections:

```text
exploration_run_start
exploration_cycle_get
exploration_cycle_active_get
exploration_cycle_events
exploration_cycle_answer
exploration_run_stop
```

Command/tool names remain snake_case; JS invocation payload fields use camelCase.
No public `cycle_next`, build-started, version-appended, verify-recorded, or decide
input exists. The Rust application service invokes internal authoring, render, and
verification services and persists those lifecycle facts itself. No candidate
promote/commit commands are added.

## Risks / Trade-offs

- Version history remains noisy by design. Filters and cycle grouping solve
  navigation without redefining persistence.
- Same-model PLAN + BUILD can preserve correlated mistakes. Deterministic verification
  and optional independent vision evaluation provide stronger boundaries than a weak
  second author.
- Separate planner calls may improve broad tasks but add latency, context transfer,
  and stale-plan risk. Evals decide.
- Coalescing drops obsolete execution work, not appended authoring history.
- A single running build limits throughput. MVP favors predictable state and cost.
- File artifacts favor local reproducibility over fleet search. A later OTEL
  exporter may index bounded metadata while large payloads remain file-backed.
