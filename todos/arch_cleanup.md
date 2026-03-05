# Architectural Cleanup

Goal: move Drydemacher from patch-driven orchestration to explicit invariant-driven flow.

## Principles

- [ ] One authoritative app flow state, not overlapping booleans
- [ ] One backend-owned context builder
- [ ] One request pipeline per user action
- [ ] Explicit invariants for question vs design vs render retry
- [ ] UI components mostly presentational
- [ ] Errors modeled as state transitions where possible, not ad hoc control flow

## Phase 1: Make State Explicit

- [ ] Define one discriminated flow state in `src/App.svelte`
  - `booting`
  - `idle`
  - `classifying`
  - `answering`
  - `generating`
  - `rendering`
  - `error`
- [ ] Map existing flags to the new phase model
  - `isGenerating`
  - `isLightReasoning`
  - `isQuestionFlow`
  - `isFreecadRunning`
  - `generationInFlight`
- [ ] Derive ECKY mode from phase only
- [ ] Derive microwave overlay visibility from phase only
- [ ] Remove impossible combined states

Acceptance:
- [ ] No overlapping generation/classification/rendering booleans remain as source of truth
- [ ] One request can occupy exactly one phase at a time

## Phase 2: Extract Session Flow Controller

- [ ] Create a controller/store module for request lifecycle
  - suggested path: `src/lib/stores/sessionFlow.ts`
- [ ] Move out of `src/App.svelte`
  - classify intent
  - question answering
  - heavy generation
  - render retry loop
  - request completion/error handling
- [ ] Keep `App.svelte` focused on composition and event wiring

Acceptance:
- [ ] `handleGenerate` is either removed or becomes a thin delegator
- [ ] Request lifecycle can be read top-to-bottom in one module

## Phase 3: Separate Domain State From View State

- [ ] Extract domain/session state
  - active thread
  - active version
  - macro code
  - UI spec
  - parameters
  - thread summary
  - pinned references
- [ ] Extract view/layout state
  - sidebar width
  - history height
  - dialogue height
  - modal visibility
  - config panel visibility
- [ ] Ensure resizing/layout logic does not mutate request pipeline state

Acceptance:
- [ ] Layout code and generation code are no longer interleaved in the same areas

## Phase 4: Backend Owns Prompt Context

- [ ] Keep prompt assembly in Rust as the only source of truth
- [ ] Frontend should send only
  - prompt
  - thread id
  - current attachments
  - current live snapshot when needed
- [ ] Backend should assemble
  - current snapshot
  - rolling summary
  - recent dialogue window
  - pinned references
- [ ] Remove duplicated prompt/context logic from frontend helpers where possible

Acceptance:
- [ ] Context shape is defined in one backend path, not split across frontend and backend

## Phase 5: Formalize Invariants

- [ ] Write explicit invariants near the orchestration code
- [ ] Enforce:
  - question replies do not mutate geometry
  - manual commit creates a new version without inheriting stale question state
  - render retry is internal recovery, not a new user request
  - one user action starts at most one active pipeline
  - current version snapshot is canonical truth
  - summary/references are memory helpers, not canonical truth

Acceptance:
- [ ] These rules exist in code comments or helper guards, not just in heads

## Phase 6: Extract Boot/Restore Flow

- [ ] Move startup flow into a dedicated coordinator
  - config load
  - model fetch
  - history load
  - last-session restore
- [ ] Make boot a linear state transition path

Acceptance:
- [ ] App startup is no longer spread across unrelated effects/helpers

## Phase 7: Extract Microwave Audio/Effects

- [ ] Move microwave hum/ding/audio context lifecycle into its own module
  - suggested path: `src/lib/audio/microwave.ts`
- [ ] Keep `App.svelte` unaware of Web Audio details

Acceptance:
- [ ] `startCooking` / `stopCooking` orchestration is not embedded in app shell code

## Phase 8: Improve Observability

- [ ] Add structured lifecycle logging
  - request started
  - classified
  - answered
  - generated
  - render retry
  - render success
  - render failure
- [ ] Include a request/run id in logs and visible status if helpful

Acceptance:
- [ ] Future “double run” bugs can be diagnosed from logs, not guesswork

## Phase 9: Pay Down Existing Reactive Debt

- [ ] Fix `src/lib/ParamPanel.svelte` reactive state warning
- [ ] Clean up obvious component warnings after flow refactor
  - `HistoryPanel.svelte`
  - `Window.svelte`
  - `ConfigPanel.svelte`
- [ ] Do not mix this with phase refactor unless necessary

Acceptance:
- [ ] Param panel warning removed
- [ ] Warning cleanup happens after architecture stops moving

## Recommended Order

- [ ] 1. Single phase model
- [ ] 2. Session flow controller/store
- [ ] 3. Boot coordinator
- [ ] 4. Domain vs view state split
- [ ] 5. Audio extraction
- [ ] 6. Observability
- [ ] 7. Warning cleanup

## Cheap-Model Work Packets

### Packet A: Phase Model
- [ ] Introduce a single phase state and derive ECKY/microwave visibility from it
- [ ] Preserve current behavior

### Packet B: Session Flow Extraction
- [ ] Move request lifecycle out of `App.svelte` into `sessionFlow.ts`
- [ ] Preserve existing Tauri command usage

### Packet C: Boot Refactor
- [ ] Extract startup restore/config/model loading into one coordinator

### Packet D: Context Boundary Cleanup
- [ ] Remove frontend duplication of prompt-context logic where backend already owns it

### Packet E: Warning Cleanup
- [ ] Fix `ParamPanel.svelte` warning first

## Definition Of Done

- [ ] `src/App.svelte` is materially smaller and mostly declarative
- [ ] Request lifecycle is readable in one place
- [ ] State transitions are explicit
- [ ] Context building is bounded and centralized
- [ ] No known duplicate-run path remains
- [ ] New feature work is cheaper than it is today
