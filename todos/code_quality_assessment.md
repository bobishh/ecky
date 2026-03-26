# Code Quality Assessment (Post-Decoupling Refactor)

Date: 2026-03-08
Status: Decoupled but not yet "Crisp"

## 1. Executive Summary
The architecture has transitioned from **monolithic/entangled** to **modular/isolated**. The most dangerous bugs (context leakage, ghost threads) have been eliminated by the introduction of immutable-like attempt records and strict active-thread checks in the orchestration layer. 

However, the "God Module" has merely been renamed to "God Controller." `requestOrchestrator.ts` contains a single ~300-line async function that is difficult to unit test and reason about due to its high cyclomatic complexity.

## 2. Assessment against Fitness Functions

### Persistence Safety: [PASS]
- Designs are initialized as `pending`.
- `finalize_generation_attempt` ensures only successful renders are marked as `success`.
- `workingCopy` edits do not touch DB until explicit commit.

### Context Isolation: [PASS]
- Async pipelines use captured `snapshotThreadId`.
- Every UI side-effect (`session.setStatus`, etc.) is guarded by `if (get(activeThreadId) === snapshotThreadId)`.

### Working-copy Integrity: [PASS]
- `sourceVersionId` is correctly managed.
- Forking creates a clean lineage.

### Boot Purity: [PASS]
- `restore.ts` handles normalization in-memory.
- Only actual repairs trigger `save_config`.
- No new designs/messages are created during boot.

### Module Boundary Discipline: [IMPROVED]
- `App.svelte` is thinner but still contains timer logic, phrase pools, and heavy initialization.
- Orchestrator depends on "injected" functions that are actually global variables in the module scope.

## 3. Technical Debt & Risks

### 1. The Orchestrator Monolith
`runRequestPipeline` is a massive "waterfall" of logic. Adding a new step (e.g., "Intermediate validation") would be extremely painful.
- **Risk**: High maintenance cost, difficult to test failure branches.
- **Recommendation**: Break into smaller, discrete steps: `classifyStep`, `llmStep`, `renderStep`, `commitStep`.

### 2. Dependency Injection Pattern
`initOrchestrator` uses a "registry" of callbacks. This is better than passing them through every function but still relies on module-level mutable state (`let viewerRef: any = null`).
- **Risk**: Hard to mock for unit tests.
- **Recommendation**: Convert the orchestrator to a Class or a Factory function that returns a closure.

### 3. The Pseudo-Queue
The `requestQueue` tracks status but does not control execution. If a user clicks "SEND" 5 times, 5 LLM requests fire concurrently.
- **Risk**: Potential race conditions or rate-limiting issues.
- **Recommendation**: Implement a simple task runner that processes requests based on their `phase` (e.g., limit concurrent LLM calls, strictly serialize FreeCAD renders).

## 4. Architectural Metrics
- `requestOrchestrator.ts` LOC: **409** (Target: < 200)
- Exported function count: **3** (Good)
- `invoke` calls outside controllers: **0** (Goal achieved)
- Direct store mutations in `App.svelte`: **Still exists** (e.g., `cookingPhrase = p`)

## 5. Next Steps
1. **Linearize the Orchestrator**: Refactor `runRequestPipeline` into a proper state machine or a chain of discrete steps.
2. **Move Timer/Phrase logic**: Extract `COOKING_PHRASES` and the "cooking timer" from `App.svelte` into a `FeedbackStore` or the `sessionStore`.
3. **Real Scheduler**: Move from `all fire at once` to a `pending -> processing` queue model.
