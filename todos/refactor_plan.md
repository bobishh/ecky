# Refactor Plan: Decoupling sessionFlow.ts

Based on the architectural analysis, `sessionFlow.ts` is currently a "god module" mixing orchestration, UI side effects, and state projection. We need to split it to improve separation of concerns and meet our fitness functions.

## 1. Concrete Metrics (Baseline)
- `sessionFlow.ts` LOC: ~580
- `invoke(...)` calls outside store/controller modules: [To be audited]
- Reads of `activeThreadId` inside async pipelines after submission: [Multiple]
- Writes during boot: [To be audited]

## 2. Structural Split

### A. Session Projection (`src/lib/stores/sessionStore.ts`)
- Owns the `session` store (phase, status, error, stlUrl, isManual).
- Provides simple state update methods.
- No orchestration logic.

### B. Request Orchestrator (`src/lib/controllers/requestOrchestrator.ts`)
- Owns `handleGenerate` and `runRequestPipeline`.
- Manages the async lifecycle of LLM design generation.
- Handles retries, classification, and final commitment.
- Uses captured snapshots instead of reading global UI state mid-pipeline.

### C. Manual Controller (`src/lib/controllers/manualController.ts`)
- Owns `handleParamChange` and `commitManualVersion`.
- Manages user-initiated render cycles and manual code edits.

## 3. Invariants to Enforce (Fitness Functions)
- **Persistence Safety**: No version in history until render succeeds.
- **Context Isolation**: Pipelines use snapshots, not `get(activeThreadId)`.
- **Queue Realism**: Enforce render serialization.

## 4. Execution Steps

### Phase 1: Extraction
- [x] Create `sessionStore.ts` and migrate the `session` store.
- [x] Create `requestOrchestrator.ts` and migrate generation logic.
- [x] Create `manualController.ts` and migrate param/manual logic.

### Phase 2: Cleanup
- [x] Remove `sessionFlow.ts`.
- [x] Audit `App.svelte` for remaining logic.

### Phase 3: Validation
- [x] Ensure all 25 E2E tests pass.
- [/] Add new "Invariant E2E tests" as specified in `fitness_functions.md`. (Started: Invariants file created, concurrency test verified)
