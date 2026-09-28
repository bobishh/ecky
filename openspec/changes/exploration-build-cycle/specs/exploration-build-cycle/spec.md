## ADDED Requirements

### Requirement: Exploration uses immutable versions as authoring truth

The system SHALL use existing immutable versions for every distinct persisted
authoring change made during exploration. Version creation SHALL NOT depend on
validation, render, or verification success. The exploration controller SHALL NOT
create separate authoring attempts, candidates, promotions, or candidate commits.

#### Scenario: Red exploratory draft is still a version

- **GIVEN** a cycle starts from version A
- **WHEN** BUILD persists changed draft B and validation fails
- **THEN** B is appended as an immutable error version and becomes head
- **AND** exact source plus raw failure remain attached to B
- **AND** no attempt, candidate, promotion, or extra commit version is created.

#### Scenario: Green exploratory draft needs no commit

- **GIVEN** changed version B rendered and verified green
- **WHEN** DECIDE completes the cycle with B chosen
- **THEN** B remains the same version created at draft persistence
- **AND** choosing B creates no additional version or lifecycle transition.

### Requirement: Exploration follows an adaptive four-stage loop

The controller SHALL progress through `PLAN`, `BUILD`, `VERIFY`, and `DECIDE`. It
SHALL select one bounded next step from current structured state and SHALL NOT execute
an unbounded pre-generated plan tail.

#### Scenario: New evidence changes the next plan

- **GIVEN** PLAN expected BUILD to satisfy minimum wall verification
- **WHEN** VERIFY reports a red minimum-wall issue on the new version
- **THEN** DECIDE records the exact evidence and selects REPLAN, ASK, or STOP
- **AND** the next PLAN uses that evidence
- **AND** no stale later BUILD executes automatically.

#### Scenario: Conclusive evidence completes without another model call

- **GIVEN** all declared acceptance criteria have deterministic green evidence
- **WHEN** DECIDE evaluates the result
- **THEN** the reducer completes the cycle with the evaluated version ref
- **AND** no planning-model call is required to restate the evidence.

### Requirement: Backend controller owns lifecycle authority

The Rust controller SHALL be the sole authority for generation-run and exploration
phase transitions, retries, budget reservation, queue arbitration, and terminal
decisions. Frontend and MCP callers SHALL submit intent or evidence inputs and SHALL
NOT assert that a build started, a version was appended, or verification succeeded.

#### Scenario: Frontend cannot manufacture a green cycle

- **GIVEN** a caller knows an existing version, snapshot, and artifact identifier
- **WHEN** it submits those identifiers without the controller performing or
  validating the owning append/render/verify operations
- **THEN** the controller rejects the transition
- **AND** no verification or completion event is persisted.

#### Scenario: UI reload loses no lifecycle authority

- **GIVEN** a backend-owned cycle is waiting, running, or awaiting input
- **WHEN** the Svelte application reloads and its local stores are recreated
- **THEN** the UI restores a compact projection from backend state
- **AND** no frontend retry counter or phase value is used as authoritative state.

### Requirement: PLAN authorizes one bounded next step

PLAN SHALL identify the exact source version, hypothesis, bounded change scope,
expected evidence, and available budget. It MAY return ASK or STOP. The reducer SHALL
reject BUILD authority when required fields are absent. PLAN content SHALL be derived
from the assembled request, current source/version, constraints, attachments, history,
answers, and exact prior evidence. Fixed generic hypothesis, scope, or evidence text
SHALL NOT authorize BUILD. PLAN SHALL remain a typed result of the authoring turn, not
a version or separate authoring entity. Its accepted action SHALL be copied to a cycle
event; only changed CAD source SHALL enter version history.

#### Scenario: Complete context produces a concrete next action

- **GIVEN** a request, current version, constraints, attachments, and prior evidence
- **WHEN** the authoring turn proposes its next action
- **THEN** PLAN names the concrete hypothesis, bounded scope, and expected evidence
  for that exact context
- **AND** Rust validates the typed `BUILD`, `ASK`, or `STOP` action before mutation
- **AND** a generic placeholder plan is rejected.

#### Scenario: Invalid plan cannot build

- **GIVEN** model output proposes BUILD without expected evidence or exact source
  version identity
- **WHEN** the reducer validates the proposal
- **THEN** BUILD is rejected before source mutation or expensive render
- **AND** the raw rejection names every missing field.

#### Scenario: Plan requests material user choice

- **GIVEN** two mounting orientations satisfy technical constraints but represent
  different product intent
- **WHEN** PLAN cannot resolve that preference from existing context
- **THEN** it emits ASK with the blocked decision
- **AND** the cycle persists the exact pending question before yielding.

### Requirement: BUILD uses append-before-validation

BUILD SHALL apply at most one bounded authoring revision and route persistence through
the existing version append service before validation or render. Identical content
SHALL create no duplicate version.

#### Scenario: Invalid build remains recoverable

- **GIVEN** PLAN authorizes one source repair
- **WHEN** the changed source is persisted but parsing fails
- **THEN** the exact changed snapshot already exists as a version
- **AND** parser status and raw diagnostic attach to that version
- **AND** DECIDE can replan from it.

#### Scenario: Provider fails before producing changed content

- **GIVEN** BUILD calls an authoring model
- **WHEN** the provider fails before any changed draft is persisted
- **THEN** no content-identical or empty version is appended
- **AND** a cycle event records the raw provider failure and route metadata.

### Requirement: VERIFY attaches layered evidence to one exact version

VERIFY SHALL bind validation, render, deterministic structural/authored checks, and
optional visual evaluation to the same version input and artifact identities. VERIFY
SHALL NOT create a version or determine history membership.

#### Scenario: Verification identity is exact

- **GIVEN** version B rendered artifact X
- **WHEN** structural and authored verification run
- **THEN** records name B, its version input digest, render snapshot, and artifact X
- **AND** evidence cannot attach to a newer moving head by implication.

#### Scenario: Deterministic failure skips subjective acceptance

- **GIVEN** authored minimum-wall verification is red
- **WHEN** VERIFY evaluates version B
- **THEN** deterministic red evidence is attached first
- **AND** a vision model cannot override that evidence to green.

### Requirement: DECIDE changes cycle state, not version identity

DECIDE SHALL select `COMPLETE`, `REPLAN`, `ASK`, `STOP`, or `COMPARE` from current
evidence. A chosen or compared version SHALL remain unchanged, including its technical
status.

#### Scenario: User chooses a red version

- **GIVEN** version B has error status and exact recoverable source
- **WHEN** the user chooses B for inspection or future work
- **THEN** the cycle may reference B
- **AND** B remains error
- **AND** no promotion, copy, or success rewrite occurs.

#### Scenario: Repair continues from exact red evidence

- **GIVEN** version B failed with repairable issue codes
- **WHEN** DECIDE selects REPLAN
- **THEN** next PLAN receives B identity and exact failing evidence
- **AND** a changed repair becomes a new immutable version C.

### Requirement: Prompt context separates stable policy from changing cycle state

The system SHALL keep language, persistence, safety, and tool invariants in a stable
prompt prefix. It SHALL provide goal, acceptance criteria, exact current version,
last evidence, remaining budget, phase, and required next output in dynamic context.
It SHALL state that no promote, commit, or finalize action exists.

#### Scenario: Repair prompt uses exact evidence

- **GIVEN** version B failed structural verification
- **WHEN** another BUILD is requested
- **THEN** dynamic context includes B identity and exact red issue codes/raw messages
- **AND** asks for the smallest bounded repair
- **AND** does not resend a stale complete plan as execution authority.

#### Scenario: Stable prompt remains cacheable

- **GIVEN** two turns use the same provider, source language, backend, and policy
- **WHEN** only current version or evidence changes
- **THEN** the stable system prefix remains byte-identical
- **AND** changing cycle state appears only after that prefix.

### Requirement: Provider turns enforce prompt-based intent categorization

Provider turns SHALL use prompt-based turn contracts that instruct the model to
categorize the user turn as `ANSWER`, `INSPECT`, `MODIFY`, or `CLARIFY`. Hardcoded word
lists or keyword-matching heuristics (WORDS) SHALL NOT be used. For `ANSWER` and
`CLARIFY`, the model SHALL invoke no mutation tools and answer directly or ask
clarifying questions. For `INSPECT`, the model SHALL invoke only read-only inspection
tools. For `MODIFY`, the model SHALL perform bounded editing, preview, and verification.
When programmatic turn policies are explicitly enforced (e.g. in test suites or
explicit evaluation runners), Rust enforces tool allowlists and non-editing mode.

#### Scenario: Status question answers without project work

- **GIVEN** an unfinished authoring turn exists in provider conversation history
- **WHEN** the user asks a status or general inquiry question
- **THEN** the prompt-based contract categorizes the turn as `ANSWER`
- **AND** the provider invokes no mutation tools and performs no project writes
- **AND** no project source changes or immutable versions are created
- **AND** the answer addresses the current question instead of resuming the repair.

#### Scenario: Inspection cannot become mutation

- **GIVEN** the user explicitly asks to inspect a current dimension
- **WHEN** the turn executes under the prompt contract or an explicit inspection policy
- **THEN** only read-only inspection tools are used
- **AND** any mutation tool call is rejected by policy
- **AND** the provider cannot change the project mirror through normal file editing.

#### Scenario: Ambiguity fails closed

- **GIVEN** a user reports an apparent geometry problem without asking to inspect or fix it
- **WHEN** intent cannot prove the requested action
- **THEN** the turn is handled as `CLARIFY`
- **AND** the provider asks one concise question without tools
- **AND** no incomplete prior mutation supplies implicit write authority.

#### Scenario: Explicit modification enables bounded authoring

- **GIVEN** the user explicitly asks to change a named dimension
- **WHEN** the turn is categorized as `MODIFY`
- **THEN** the normal inspect, edit, preview, and verify surface is available
- **AND** that authority applies only to the current user turn
- **AND** every changed draft still follows normal immutable-version semantics.

### Requirement: Model routing is evidence-driven and auditable

The system SHALL record prompt version and selected provider/model/reasoning route for
model-backed cycle events. Default routing SHALL use deterministic checks where
possible and one capable authoring route for PLAN, BUILD, and repair. A different
planner, implementer, or visual verifier SHALL ship only after representative evals
show required quality at an acceptable latency/cost trade-off.

#### Scenario: Cheap model does not replace deterministic verification

- **GIVEN** the backend can evaluate a declared structural check
- **WHEN** VERIFY runs
- **THEN** the deterministic checker owns that result
- **AND** no model route may override it.

#### Scenario: Route experiment is comparable

- **GIVEN** an alternate planner, author, effort, or vision route is proposed
- **WHEN** it is evaluated
- **THEN** the same representative fixtures and acceptance criteria are replayed
- **AND** completion, first-build green, repair success, invalid output, version count,
  latency, tokens, and cost are recorded
- **AND** only one routing variable changes per comparison.

### Requirement: Interactive pending builds use latest-wins coalescing

The system SHALL run at most one expensive build per cycle at a time. Unstarted
interactive rebuild requests for the same working target SHALL coalesce to the newest
exact input. Explicit controller BUILD steps SHALL NOT be silently coalesced.

#### Scenario: Rapid parameter applies avoid stale FIFO execution

- **GIVEN** build A is running
- **WHEN** interactive requests B, C, and D arrive for the same working target
- **THEN** only D remains pending for execution
- **AND** already-persisted versions for B and C remain in immutable history
- **AND** D starts after A completes or cancels.

#### Scenario: Obsolete completion keeps evidence but not projection

- **GIVEN** running build A becomes obsolete after pending request D
- **WHEN** A completes successfully
- **THEN** A's evidence remains attached to A's version
- **AND** A does not replace the active viewport projection
- **AND** version head semantics remain based on append order.

### Requirement: ASK suspends execution with decision context

The system SHALL persist an ASK event with exact question, blocked decision, cycle
identity, and current version ref before yielding. Conversation text SHALL NOT be the
authoritative pending-decision record.

#### Scenario: Answer resumes same decision

- **GIVEN** a cycle waits for mounting-hole spacing
- **WHEN** the user answers after restart
- **THEN** the answer attaches to the persisted pending question
- **AND** PLAN resumes from the same cycle and version state.

### Requirement: Budget and stop state bound exploration

The system SHALL reserve budget before model or expensive build work and SHALL stop
automatic progression when the relevant budget is exhausted. Stopping SHALL preserve
all versions and evidence.

#### Scenario: Build budget prevents another render

- **GIVEN** a cycle used its exact-build budget
- **WHEN** another BUILD is proposed
- **THEN** the reducer rejects automatic execution
- **AND** exposes STOP, ASK, or explicit user budget extension
- **AND** creates no synthetic final version.

### Requirement: Restart restores exploration without auto-resume

The system SHALL restore cycle phase, objective, acceptance criteria, budget, version
refs, evidence refs, route metadata, and pending question after restart. It SHALL NOT
automatically resume model or expensive render work whose liveness cannot be proven.

#### Scenario: Restart interrupts in-flight build safely

- **GIVEN** a cycle was building when the process stopped
- **WHEN** the project reopens and no worker can be proven alive
- **THEN** the cycle records interrupted work with raw diagnostic
- **AND** existing versions and evidence remain unchanged
- **AND** the user may explicitly retry from the same source version.

### Requirement: UI projects cycles over ordinary version history

The workbench SHALL show every immutable version in primary history with its status,
show successful/printable versions through filters, and group cycle state/evidence
without a candidate tier or separate global agent status bar. When a provider-owned
turn is active before its first public activity event arrives, the dialogue SHALL
project a receipt/working fallback from that backend runtime state. Exact provider
activity, terminal state, or raw failure SHALL replace the fallback.

Provider dialogue SHALL expose the original user text while keeping transport-only
turn policy wrappers private. Rust SHALL preserve the original input in durable
provider history and unwrap only an exact current or legacy wrapper when projecting
provider-owned history.

#### Scenario: Provider turn policy stays out of user dialogue

- **GIVEN** a provider turn was sent with an internal turn policy wrapper
- **WHEN** live, persisted, or reloaded dialogue is projected
- **THEN** Ecky shows the original user text
- **AND** policy instructions and wrapper markers remain hidden
- **AND** ordinary user-authored marker text remains unchanged.

#### Scenario: Active provider turn acknowledges receipt before thinking arrives

- **GIVEN** the backend accepted a provider turn and reports its exact active turn ID
- **AND** no public provider activity has arrived yet
- **WHEN** the dialogue projects that runtime packet
- **THEN** Ecky shows that the message was received and work started
- **AND** the first provider thinking/activity event replaces the fallback
- **AND** terminal failure removes it and exposes the raw provider error.

#### Scenario: Exploration does not create hidden history classes

- **GIVEN** one cycle creates six changed drafts
- **WHEN** the user opens primary history
- **THEN** six versions appear with their actual statuses
- **AND** no candidate promotion or commit is required to reveal them.

#### Scenario: Red head preserves last good viewport

- **GIVEN** successful version A is visible
- **WHEN** newer version B fails
- **THEN** B is version head and its raw diagnostic is inspectable
- **AND** A's successful render may remain the active viewport projection
- **AND** the UI identifies that head and viewport refer to different versions.

#### Scenario: Background parameter render updates the selected viewport

- **GIVEN** a parameter edit appended version B and its render is pending
- **AND** the viewport still displays version A's successful artifact
- **WHEN** Rust reports that B rendered successfully
- **THEN** the UI loads B's exact persisted runtime without another render or version append
- **AND** a completion for a version or thread no longer selected cannot replace the viewport.

#### Scenario: Background parameter render failure retains the last good viewport

- **GIVEN** parameter version B is selected while version A's artifact remains visible
- **WHEN** Rust reports that B failed rendering
- **THEN** the UI exposes B's raw backend diagnostic
- **AND** A's successful runtime remains visible.

### Requirement: Provider turns produce file-backed LLM eval evidence

The system SHALL persist each completed Agy managed-provider turn as strict data-only
EDN plus a deterministic Markdown report. Its schema SHALL remain provider-neutral so
other managed providers can use the same writer. The artifact SHALL retain ordered model
and tool activity, bounded safe tool inputs/results, route identity, timing,
terminal outcome, classified intent, effective capability policy, policy violations,
and immutable versions created during the turn. The system SHALL
NOT require a JSON log format or database-backed log platform for eval replay.

#### Scenario: Tool trajectory survives restart

- **GIVEN** an Agy managed-provider turn emits inspect, command, edit, and verification steps
- **WHEN** the turn terminates
- **THEN** `run.edn` and `trajectory.edn` retain their exact order and turn identity
- **AND** `report.md` records deterministic behavioral and outcome metrics
- **AND** reopening the files requires no live provider process or in-memory journal.

#### Scenario: Eval artifact redacts secrets and bounds output

- **GIVEN** a tool input contains a token and its output exceeds the payload bound
- **WHEN** trajectory evidence is persisted
- **THEN** the token value is replaced by a redaction marker
- **AND** the oversized output records a clipped value plus full SHA-256 digest
- **AND** no JSON eval artifact is created.

#### Scenario: Paired route comparison uses the same case

- **GIVEN** two completed run artifacts share one case and differ in one route variable
- **WHEN** comparison runs
- **THEN** the Markdown comparison reports completion, first-build-green,
  red-to-green repair, unnecessary versions, tool repetition, latency, tokens, and cost
- **AND** comparison rejects mismatched cases or multiple changed route variables.

#### Scenario: Streaming tool states count once

- **GIVEN** one provider tool step emits `active` and `done` trajectory updates with
  the same stable step identity
- **WHEN** deterministic trajectory scoring runs
- **THEN** the report records one tool call
- **AND** the state transition does not count as an adjacent repeated tool call.
