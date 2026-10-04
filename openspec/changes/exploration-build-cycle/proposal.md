# Change: Exploration Build Cycle

## Why

Ecky already has lossless immutable version history. Every distinct persisted
authoring change is a version before validation or render, regardless of whether
the result later succeeds or fails. Version status and attached evidence describe
the outcome; exploration must not add a second attempt/candidate/commit lifecycle
that competes with this truth.

The missing product boundary is bounded reasoning. Long CAD work still needs a
restart-safe loop that chooses one useful revision, builds it, evaluates exact
evidence, and decides whether to stop, ask, or revise again. A static multi-step
plan becomes stale after the first compiler, geometry, or verification result.

## What Changes

- Add one adaptive four-stage controller loop:
  `PLAN -> BUILD -> VERIFY -> DECIDE`.
- Treat the loop as orchestration over immutable versions, not as a new authoring
  record hierarchy.
- In `PLAN`, select one bounded next change from current version state, user goal,
  acceptance criteria, prior evidence, and remaining budget.
- In `BUILD`, persist every distinct changed draft through the existing append
  path. That append is immediately the new version and head. No success gate,
  promotion, or manual commit exists.
- In `VERIFY`, attach validation, render, structural, authored, and optional visual
  evidence to that same version.
- In `DECIDE`, complete, ask, stop, compare, or start another plan from observed
  evidence. The decision may reference a chosen version but never changes version
  identity or rewrites its status.
- Persist compact cycle state and append-only cycle events so restart retains the
  objective, acceptance criteria, budget, pending question, version refs, and
  decisions.
- Make the Rust controller the only lifecycle authority. Frontend code may submit,
  answer, stop, and project events, but SHALL NOT own retry loops, budget,
  phase transitions, queue arbitration, or completion decisions.
- Project background parameter-render outcomes onto the still-selected version:
  load its successful persisted runtime or expose its raw failure while retaining
  the last-good viewport, without another render or version append.
- Run at most one expensive build for a cycle at a time. Coalesce unstarted
  interactive rebuild requests to the latest exact input while retaining every
  draft version already appended by normal authoring.
- Define a lean prompt contract and an evidence-driven model routing policy.
- By default, use prompt-based provider turns where the prompt defines the turn policy contract
  (`ANSWER`, `INSPECT`, `MODIFY`, `CLARIFY`) and the LLM itself categorizes the turn.
  Eliminate brittle hardcoded word lists (WORDS); ambiguous intent never grants
  write authority.
- Implement the opt-in global `experimental-jev-routing` extension: one Rust-owned
  Jev intent/prompt route for every application-owned API, Codex, Agy, and managed
  MCP request. Provider-specific model selection remains within its verified
  ceiling. Native hook enforcement is separate from Jev activation. Routing
  preserves selected action under moderate confidence and projects an accepted
  classification probabilities from a separate request/message result. It remains off by
  default and exploration/version ownership stays unchanged. Existing
  Codex-only fixture proof does not establish global behavior, live classifier
  quality, cost savings, or universal native-tool coverage by itself.
- Persist completed provider-turn trajectories as bounded strict-EDN files plus
  human-readable Markdown reports so model behavior can be evaluated without a
  database-backed log product. Tool order, safe inputs/results, timing, terminal
  outcome, and version/verification outcomes remain replayable after restart.
- Extend the existing writer to queued Codex turns, including failure, interruption,
  and app-server exit. Retry failed persistence without acknowledging lost evidence.
  Preserve useful Agy answers alongside terminal provider errors. Unfinished Codex
  capture is not yet durable across a full Ecky process exit.

## Relationship To Existing Changes

`lossless-version-history` is authoritative for persistence:

- every distinct persisted source/draft change is an immutable version;
- head is the latest append, independent of status;
- validation, render, and verification outcomes attach to that version;
- successful versions are a filter, not a different lifecycle;
- there is no generic commit/finalize authoring operation.

This change adds orchestration metadata only. It does not revise those semantics.

## Out Of Scope

- A separate attempt, candidate, promotion, or candidate-commit entity.
- Up-front generation of a complete executable plan.
- Hiding, deleting, or squashing failed, superseded, or exploratory versions.
- Parallel geometry builds or distributed workers.
- Automatic subjective design ranking without declared evidence.
- Hard-coding one vendor model to each stage before representative evals.
- New CAD language operations, constraints, or feature semantics.
- A second browser-owned generation or retry state machine beside the Rust
  controller.

## Proof Gates

- Six changed exploratory drafts create six immutable versions. Failed and
  successful statuses remain attached to their versions; no promotion or commit
  step creates an extra version.
- A red version remains head and inspectable while the last good render may remain
  the active viewport projection.
- Verification evidence names the exact version input and artifact digest it
  evaluated.
- New evidence changes the next `PLAN`; no stale pre-generated build tail runs.
- Three queued interactive rebuild requests produce the running build plus one
  newest pending build, while already-appended draft versions remain recoverable.
- Restart restores cycle phase, objective, acceptance criteria, budget, referenced
  versions, last evidence, and pending user question without resuming expensive
  work automatically.
- Model-routing experiments compare completion quality, red-to-green repair rate,
  latency, token use, and cost before any non-default route ships.
- A provider turn with tool activity produces no JSON eval artifact: its run
  directory contains strict `run.edn`, ordered `trajectory.edn`, and `report.md`.
  Secret-shaped fields are redacted and oversized payloads remain digest-addressed.
- A status/explanation message cannot mutate project source or append a version.
  An inspection message can use bounded reads but cannot write. Only explicit
  modification intent enables authoring tools.
