# Codex Provider Integration Specification

## ADDED Requirements

### Requirement: Provider mode is explicit configuration

Settings SHALL expose `API KEY`, `MCP`, and `PROVIDER`. Provider SHALL select an
adapter; `CODEX` is the only initial option.

#### Scenario: User selects Codex provider

- **GIVEN** an Ecky thread
- **WHEN** user selects `PROVIDER` and `CODEX` and saves
- **THEN** config persists `provider:codex`
- **AND** Dialogue routes new text to Codex instead of API or MCP

### Requirement: Ecky threads own durable provider conversations

Each Ecky thread SHALL own one durable normalized provider timeline per adapter and
at most one current provider execution cursor. Superseded cursors SHALL remain as
lineage. The provider-global conversation index SHALL NOT be exposed.

#### Scenario: First provider message creates owned conversation

- **GIVEN** provider mode and an Ecky thread without binding
- **WHEN** user sends first non-empty message
- **THEN** Ecky starts one persisted Codex thread
- **AND** stores returned id against that Ecky thread
- **AND** sends message into that id
- **AND** does not list or select existing Codex threads

#### Scenario: Returning to provider mode

- **GIVEN** an Ecky thread already bound to Codex
- **WHEN** user switches away and later returns to Provider
- **THEN** Ecky renders its locally persisted provider timeline without resuming a writer
- **AND** creates no replacement while loading history or handling a writer conflict

#### Scenario: Stored Codex cursor has another active writer

- **GIVEN** finished provider turns are persisted under an Ecky thread
- **AND** another Codex client owns the current external writer
- **WHEN** Ecky dispatches a queued prompt
- **THEN** Ecky read-only backfills any available finished turns
- **AND** retains the same current binding, external thread id, and provider history
- **AND** keeps the same queued prompt at the FIFO head with the precise raw provider error
- **AND** schedules a delayed retry using the existing queue recovery path
- **AND** does not create a replacement thread, unsubscribe, kill, or interrupt the other client

#### Scenario: Foreign writer conflict resolves

- **GIVEN** a queued prompt remains bound to the same Codex external thread after an active-writer error
- **WHEN** a later bounded retry finds that the foreign writer has released the thread
- **THEN** Ecky resumes and delivers the queued prompt to that same external thread id
- **AND** the Ecky binding and finished provider history remain continuous

#### Scenario: Start fails

- **WHEN** Codex thread/start fails
- **THEN** no binding is saved
- **AND** raw provider body is visible
- **AND** prompt remains retryable

### Requirement: Bootstrap carries Ecky identity and context

Start and process-generation resume SHALL inject versioned developer instructions,
live Ecky MCP configuration, canonical compacted thread summary, recent dialogue,
design identity, and artifact identity.

The MCP override SHALL use an integration-owned server name that cannot merge with
the user's existing `ecky_mcp` stdio configuration. It SHALL be required, so start
fails rather than silently continuing without CAD tools.

#### Scenario: API work moves to Codex

- **GIVEN** an Ecky thread with API dialogue and CAD target
- **WHEN** first Codex provider message starts its conversation
- **THEN** bootstrap identifies exact Ecky thread and current design
- **AND** carries bounded API dialogue context
- **AND** tells Codex to inspect authoritative CAD state through Ecky MCP

#### Scenario: User already configured Ecky MCP in Codex

- **GIVEN** global Codex config contains `ecky_mcp` using stdio
- **WHEN** Ecky starts or resumes its Provider conversation
- **THEN** Ecky configures Streamable HTTP under private name `ecky_provider_mcp`
- **AND** no stdio/HTTP transport fields merge

### Requirement: Codex works in the canonical project mirror

For a thread with a committed version, Ecky SHALL export through the existing
project-folder boundary and use its returned canonical folder as Codex cwd. Blank
threads MAY use a deterministic empty per-thread workspace until a version exists.

#### Scenario: Thread title differs from design title

- **GIVEN** project-folder export has bound a thread to a canonical mirror
- **WHEN** Ecky starts or resumes Codex Provider mode
- **THEN** Codex cwd equals the returned bound folder
- **AND** `model.ecky` and `ecky-project.edn` are present
- **AND** Ecky does not independently derive a competing folder from thread title

### Requirement: Provider context returns to canonical Ecky context

Finished provider user/assistant turns SHALL be durably normalized into
`agent_provider_messages`. Recent provider conversation SHALL update bounded
`threads.summary` for API/MCP handoff. Persisting new provider messages SHALL
atomically advance the owning thread's `updated_at`. The first non-empty provider
user message SHALL replace only the default `Untitled design` title with a bounded
prompt prefix. Existing provider history SHALL be backfilled to the same projection.

#### Scenario: Codex work moves to API or MCP

- **GIVEN** completed Codex dialogue on an Ecky thread
- **WHEN** user selects API or MCP and sends next message
- **THEN** existing context assembler includes recent Codex handoff
- **AND** next runtime knows current target and recent decisions

#### Scenario: Provider-only thread enters Projects inventory

- **GIVEN** an Ecky thread has blank model source and no CAD version
- **WHEN** a provider user or assistant message is durably persisted
- **THEN** the owning thread `updated_at` advances in the same transaction
- **AND** history no longer classifies the thread as a reusable blank
- **AND** the first non-empty user prompt names a still-default thread
- **AND** a manual thread title is never replaced

### Requirement: History is cursor paged

Dialogue SHALL fetch at most 30 locally persisted provider messages per page and use
opaque cursors. Provider reconciliation SHALL run read-only in the background.
Ecky and provider events SHALL render in one chronological timeline; provider loading
SHALL NOT replace already visible Ecky messages or versions.

#### Scenario: Dialogue opens while provider is unavailable or writer-locked

- **GIVEN** Ecky already persisted finished provider turns
- **WHEN** Dialogue opens
- **THEN** the local provider page renders immediately
- **AND** no provider writer activation is required
- **AND** background reconciliation failure does not replace or clear local history

#### Scenario: Older history loads

- **WHEN** user selects `SHOW OLDER MESSAGES`
- **THEN** Ecky passes returned opaque cursor once
- **AND** loads both available Ecky and provider pages
- **AND** prepends stable-id deduplicated messages without moving the scroll anchor

#### Scenario: Provider snapshot arrives after Ecky history

- **GIVEN** Dialogue already shows Ecky versions
- **WHEN** the owned provider snapshot loads
- **THEN** versions remain visible in the same timeline
- **AND** no source replaces or flashes over the other

#### Scenario: Provider image attachment survives history reload

- **GIVEN** a provider user message includes an image attachment
- **WHEN** the accepted turn is persisted and Dialogue later reloads its local page
- **THEN** Ecky renders the original image beside that user message
- **AND** read-only Codex backfill may restore missing image metadata from `image` or `localImage` input blocks
- **AND** an attachment-free later projection does not erase an already persisted image

#### Scenario: Codex generated image survives history reload

- **GIVEN** a completed owned Codex turn contains an `imageGeneration` output
- **WHEN** Ecky backfills and later reloads its local provider page
- **THEN** Ecky persists the generated image as an assistant timeline attachment
- **AND** Dialogue renders that image beside its generated-sketch message

#### Scenario: Ecky sketch MCP result survives history reload

- **GIVEN** a completed owned Codex turn contains an Ecky MCP tool result with a `SketchDocument`
- **WHEN** Ecky backfills and later reloads its local provider page
- **THEN** Ecky persists one concise assistant result with the sketch document identity and sketch count
- **AND** Dialogue renders that result without replacing the current canonical `.ecky` model
- **AND** incomplete sketch tool calls do not appear as completed results

#### Scenario: User searches versions

- **WHEN** user selects `VERSIONS` or enters timeline search text
- **THEN** Dialogue filters the merged timeline locally
- **AND** matching version title, name, response, or content remains visible

### Requirement: Provider source evidence opens the bound model

Dialogue SHALL present absolute Markdown references to the current bound
`model.ecky:LINE` as code controls while preserving the raw durable provider
transcript. It SHALL omit standalone internal `messageId` and `modelId` evidence
lines from visible/copyable answer text.

#### Scenario: Final answer cites current model source

- **GIVEN** a provider answer contains `[model.ecky](ABSOLUTE_BOUND_PATH:110)`
- **WHEN** user activates that reference
- **THEN** the existing Code window loads the current thread's bound source
- **AND** line 110 becomes the active editor line
- **AND** standalone internal message/model IDs are not visible

#### Scenario: Referenced source cannot be loaded or no longer matches

- **WHEN** bound-source lookup fails or its current file differs from the cited path
- **THEN** Code remains closed
- **AND** raw source error appears inline with that answer
- **AND** Ecky does not open an arbitrary local file or browser

### Requirement: Queue and live controls are exact

Normal submit during active work SHALL append durable FIFO. `STEER` SHALL target exact
active turn. `STOP` SHALL interrupt exact active turn without discarding FIFO.

#### Scenario: User selects a Codex model

- **WHEN** user persists a nonblank Codex model id in Provider settings
- **THEN** start/resume receives that model where supported
- **AND** every next `turn/start` carries exact `model`
- **AND** blank selection delegates to the Codex default

#### Scenario: Provider delivery is slow

- **WHEN** user submits a provider prompt
- **THEN** a local `QUEUED` user item paints immediately
- **AND** the composer accepts another prompt without waiting for app-server delivery
- **AND** backend enqueue returns before asynchronous turn dispatch
- **AND** accepted provider transcript replaces the optimistic copy

#### Scenario: Idle queue is dispatchable

- **GIVEN** no Codex turn is active
- **WHEN** a provider prompt is durably enqueued
- **THEN** enqueue wakes dispatch immediately
- **AND** dispatcher atomically claims that queue row
- **AND** a slow delivery in another Ecky thread cannot block it behind a global lock
- **AND** periodic polling is recovery, not primary delivery

#### Scenario: Agent work streams

- **GIVEN** an active owned Codex turn
- **WHEN** app-server emits public commentary, reasoning-summary, plan, or tool-item progress
- **THEN** Dialogue renders one expandable transient `WORKING` trace for activity in event order
- **AND** agent-message deltas append to their stable item id
- **AND** agent-message speech remains an ordinary assistant reply outside `WORKING`
- **AND** raw reasoning text and terminal stdout are not exposed
- **AND** Dialogue does not reload cursor-paged transcript turns for each delta
- **AND** terminal state replaces transient projection with persisted transcript

#### Scenario: Codex desktop is running without the owned task open

- **GIVEN** Ecky's app-server owns the bound provider conversation
- **AND** Codex desktop IPC exists but no desktop window renders that conversation
- **WHEN** a queued provider prompt becomes dispatchable
- **THEN** Ecky sends it through its own app-server
- **AND** Dialogue never asks the user to open a Codex task
- **AND** the desktop IPC socket does not alter delivery routing

#### Scenario: Provider transcript timestamps collide

- **WHEN** app-server returns newest-first turns, item order varies, or timestamps match
- **THEN** backend projects turns oldest-first and each user item before its assistant items
- **AND** accepted user items are not labeled `QUEUED`

#### Scenario: Compaction occurs during active turn

- **WHEN** `thread/compacted` arrives
- **THEN** active turn remains active
- **AND** queue does not advance

#### Scenario: App-server omits turn/completed

- **GIVEN** a matching turn is active
- **WHEN** app-server reports `thread/status/changed` with `idle`
- **THEN** runtime marks the turn terminal
- **AND** FIFO may advance without waiting forever

#### Scenario: Turn fails

- **WHEN** turn/start fails
- **THEN** FIFO head becomes failed with raw body
- **AND** prior transcript remains visible
- **AND** retry/remove actions remain available

### Requirement: Core persistence is provider-neutral

Bindings, binding lineage, normalized finished messages, and FIFO SHALL namespace
external ids by adapter and SHALL NOT encode Codex schema in their generic records.

#### Scenario: Future adapter is added

- **WHEN** Claude Code adapter implements lifecycle capabilities
- **THEN** it can reuse Ecky ownership, handoff, and FIFO records
- **AND** Codex-specific UI or migration is not required

### Requirement: Global configuration errors stay threadless

Configuration persistence failures SHALL render as global application notifications,
not as the current Ecky thread's dialogue or mascot state.

#### Scenario: Provider config save fails

- **WHEN** Settings save returns a raw backend error
- **THEN** Settings shows the raw error
- **AND** the notification scope is `ECKY APP` with no thread id
- **AND** current thread Dialogue has no config-error bubble
