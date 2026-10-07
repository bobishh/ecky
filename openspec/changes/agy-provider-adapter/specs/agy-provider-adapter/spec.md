# Antigravity Provider Adapter Specification

## ADDED Requirements

### Requirement: Provider settings select Antigravity

Settings SHALL show `CODEX` and `AGY` under Provider. Selecting AGY SHALL persist
`provider:agy` and route Dialogue messages through the Antigravity adapter.

#### Scenario: User selects AGY

- **GIVEN** an Ecky thread
- **WHEN** user selects `PROVIDER`, selects `AGY`, and saves
- **THEN** config persists `provider:agy`
- **AND** Dialogue offers `SEND TO AGY`
- **AND** new text invokes the Agy provider adapter

#### Scenario: User selects an Agy model

- **WHEN** user enters an Agy model id and saves Provider settings
- **THEN** config persists that id independently from the Codex model
- **AND** the next Agy process receives exact `--model <id>`
- **AND** blank selection delegates to the provider default

### Requirement: Provider ownership survives switching

Each Ecky thread SHALL own at most one conversation per provider. Switching modes or
providers SHALL preserve every existing binding and SHALL NOT expose provider-global
conversation indexes.

#### Scenario: Codex and Agy coexist

- **GIVEN** one Ecky thread owns a Codex conversation
- **WHEN** user switches to AGY and sends its first message
- **THEN** Ecky creates one Agy conversation for the same Ecky thread
- **AND** the Codex binding remains unchanged
- **AND** returning to Codex resumes its exact conversation id

### Requirement: Opening an Ecky thread reads its durable Ecky timeline

Before provider process work, Ecky SHALL read the durable local timeline bound to the
open Ecky thread. Opening SHALL NOT activate Codex or Agy writers. Provider process
resume remains lazy until real delivery. A missing binding SHALL remain lazy until
the first real user message.

#### Scenario: Bound Codex thread opens

- **GIVEN** an Ecky thread owns a Codex conversation
- **WHEN** that Ecky thread becomes active under `provider:codex`
- **THEN** Ecky reads locally persisted Codex history without `thread/resume`
- **AND** background read-only reconciliation may run after local history renders
- **AND** no takeover control or foreign-thread index is shown

#### Scenario: Bound Agy thread opens

- **GIVEN** an Ecky thread owns an Agy conversation
- **WHEN** that Ecky thread becomes active under `provider:agy`
- **THEN** Ecky reads locally persisted Agy history
- **AND** no Agy process starts until real queued delivery
- **AND** no fake user prompt is synthesized
- **AND** `--conversation` is not invoked merely to acquire ownership

#### Scenario: Provider is unavailable while Dialogue opens

- **WHEN** provider reconciliation or process startup would fail
- **THEN** already persisted local history remains visible
- **AND** Dialogue opening is not blocked by that provider error

### Requirement: Agy starts and resumes through bidirectional stream JSON

Ecky SHALL use Antigravity CLI `>=1.1.15`, one NDJSON process per active conversation,
and one user event per FIFO turn. New conversations SHALL bind only to ids reported
by `init`; existing conversations SHALL resume with `--conversation`.

#### Scenario: First Agy message

- **WHEN** first text is delivered
- **THEN** Ecky starts `agy` with both stream-json formats in the canonical cwd
- **AND** sends the prompt as a user event on stdin
- **AND** persists the `init.conversation_id`
- **AND** does not list or take over foreign Agy conversations

#### Scenario: Ecky restarts during an accepted Agy delivery

- **GIVEN** an Agy FIFO row was durably marked `sending`
- **WHEN** a new Ecky process recovers its database
- **THEN** Ecky verifies the durable provider run identity before signaling any process
- **AND** terminates the exact orphaned Agy process group when identity still matches
- **AND** the row becomes failed with actionable raw recovery detail
- **AND** Ecky does not automatically replay it through `agy --conversation`
- **AND** the user can inspect/remove the failed row before another explicit delivery

#### Scenario: Ecky exits cleanly with active Agy work

- **GIVEN** an Agy turn owns a CLI process and descendant tool processes
- **WHEN** Ecky quits or restarts
- **THEN** Ecky terminates its isolated Agy process group before Tauri exits
- **AND** no provider or tool descendant continues after Ecky exits

#### Scenario: Installed CLI is too old

- **GIVEN** `agy --version` reports lower than `1.1.15`
- **WHEN** delivery starts
- **THEN** no binding is created
- **AND** raw actionable version detail is shown
- **AND** queued prompt remains retryable

### Requirement: Agy receives Ecky bootstrap and workspace tools

The first Agy turn SHALL contain versioned Ecky identity, thread id/title, canonical
handoff, canonical cwd, and authoring workflow. Ecky SHALL materialize a workspace
`.agents/plugins/ecky-provider/` plugin using the live HTTP endpoint under
`ecky_mcp` without changing global Agy configuration.

#### Scenario: API or Codex work moves to Agy

- **GIVEN** canonical thread summary and a project mirror
- **WHEN** Agy receives its first turn
- **THEN** the prompt identifies current target and recent decisions
- **AND** the Agy cwd is the canonical mirror
- **AND** the workspace plugin contains `plugin.json`, `mcp_config.json`, and a tool guide
- **AND** MCP config uses `serverUrl`, not an stdio `url`
- **AND** the workspace `ecky_mcp` entry overrides any same-named global entry
- **AND** its endpoint pre-binds the exact Ecky thread before the first tool call
- **AND** the prompt tells Agy to read the tool guide and routed MCP guide resources before editing
- **AND** the assigned provider thread does not call `thread_borrow`
- **AND** user-facing source evidence uses `[model.ecky](ABSOLUTE_BOUND_PATH:LINE)`
- **AND** user-facing answers omit internal `messageId` and `modelId` fields

### Requirement: Agy progress and history do not hang Dialogue

Ecky SHALL read stdout line-by-line, project bounded public progress, persist terminal
turns, and cursor-page its local Agy transcript. It SHALL not wait for process exit
between turns.

#### Scenario: Agy streams work

- **WHEN** Agy emits response deltas and tool steps
- **THEN** Dialogue shows one `WORKING` status event containing the latest public event
- **AND** action events accumulate in exact arrival order inside an expandable `WORKING` trace
- **AND** assistant response deltas remain ordinary assistant speech outside `WORKING`
- **AND** active `WORKING` can be expanded without stopping the turn
- **AND** raw terminal output is omitted
- **AND** a successful terminal result moves the trace into a collapsed history event before the persisted final answer
- **AND** an interrupted or failed terminal result exposes the full ordered trace by default

#### Scenario: Older Agy history loads

- **WHEN** user selects `SHOW OLDER MESSAGES`
- **THEN** Ecky requests at most 30 local transcript rows by opaque cursor
- **AND** merges them with Ecky versions without replacing the timeline

#### Scenario: Agy image attachment survives history reload

- **GIVEN** an accepted Agy user message includes an image attachment
- **WHEN** its FIFO row completes and Dialogue later reloads local history
- **THEN** the persisted user message still renders that image
- **AND** deleting the completed queue row does not delete attachment metadata

#### Scenario: Agy performs long-running work

- **WHEN** one turn emits many activity updates, repeats tool activity, or runs longer than 10 minutes
- **THEN** Ecky keeps the exact child turn running
- **AND** updates repeated `step_index` progress in place
- **AND** stops only on provider result, explicit user `STOP`, process exit, or transport error

#### Scenario: MCP endpoint changes between Agy turns

- **GIVEN** an idle warm Agy child was started with an older Ecky MCP endpoint
- **WHEN** the next delivery sees a different live endpoint
- **THEN** Ecky discards the stale child
- **AND** resumes the same Agy conversation in a new child
- **AND** its workspace plugin pre-binds the same Ecky thread at the new endpoint

### Requirement: Agy queue and controls reflect real capabilities

Submit SHALL durably enqueue and return immediately. Agy SHALL expose stop and SHALL
not expose steer until its protocol documents an exact active-turn steering command.
Agy SHALL omit `VOICE` while stream-json supports only text blocks and SHALL NOT
route voice recordings to standalone NVIDIA Speech.

#### Scenario: Agy has no integrated voice input

- **GIVEN** `provider:agy` uses text-only stream-json input
- **WHEN** Dialogue opens
- **THEN** the text composer and `SEND TO AGY` remain available
- **AND** `VOICE` is absent
- **AND** direct standalone STT calls fail before NVIDIA provider I/O

#### Scenario: User sends while Agy works

- **WHEN** a turn is active and user submits another prompt
- **THEN** a local queued item appears immediately
- **AND** FIFO delivers it after the result event
- **AND** composer remains usable

#### Scenario: User stops Agy

- **GIVEN** an exact active Agy turn
- **WHEN** user selects `STOP`
- **THEN** Ecky sends SIGINT to its isolated provider process group
- **AND** preserves queued messages
- **AND** next delivery resumes the same conversation

#### Scenario: Agy is active

- **THEN** Dialogue shows `STOP`
- **AND** Dialogue does not show `STEER`

### Requirement: Provider activity and authored versions remain scannable

Dialogue SHALL project active provider work as one current-event status rather than
one chat bubble per tool or progress delta. It SHALL retain the full ordered turn trace.
Successful turns SHALL collapse that trace and show the persisted answer; interrupted
or failed turns SHALL expose the trace in its original order. Versions created during provider work SHALL
remain separate timeline events, visually distinct from assistant prose. Provider
controls SHALL carry the same primary action weight as send while preserving capability
truth: stop uses the destructive treatment and steer uses the provider accent treatment.

#### Scenario: Provider emits several public updates

- **GIVEN** one active provider turn
- **WHEN** several response and tool updates arrive
- **THEN** Dialogue replaces the visible `WORKING` status copy with each latest action event
- **AND** prior action events remain accumulated without being rendered as chat bubbles
- **AND** assistant speech remains a normal chronological reply

#### Scenario: User interrupts provider work

- **WHEN** the active turn ends as interrupted or failed
- **THEN** Dialogue shows every accumulated public event in original order
- **AND** the trace is expanded by default

#### Scenario: Provider completes with an answer

- **WHEN** the active turn succeeds and its persisted answer arrives
- **THEN** Dialogue renders the accumulated trace collapsed
- **AND** renders the final answer as ordinary transcript immediately after it

#### Scenario: Provider creates a version while working

- **WHEN** a version enters the merged Dialogue timeline
- **THEN** it renders as a full-width version event rather than assistant chat prose
- **AND** current, discarded, tuning, and restore actions retain their existing meaning

#### Scenario: Provider turn supports controls

- **WHEN** stop or steer is available
- **THEN** its control matches the send control height and emphasis
- **AND** stop is filled red
- **AND** steer is filled with the provider accent
- **AND** an unsupported control remains absent

### Requirement: Warm Agy turns do not duplicate canonical context

The first turn and a process resume SHALL receive bounded canonical handoff. A warm
continuation in the same process SHALL receive only the new user message plus a short
pre-bound-session reminder.

#### Scenario: User continues a warm Agy conversation

- **GIVEN** the owned Agy process is idle and its model and MCP endpoint still match
- **WHEN** the next FIFO prompt starts
- **THEN** the outbound event omits the prior canonical handoff
- **AND** includes the new user message exactly once
