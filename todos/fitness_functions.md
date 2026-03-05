# Architectural Fitness Functions

This document defines the invariants and metrics used to measure the structural integrity of the Drydemacher codebase. These are the rules we must not break, even when the void is calling.

## 1. Persistence Safety
*   **Invariant**: A generated design MUST NOT be stored in history until the FreeCAD render succeeds.
*   **Invariant**: A failed retry MUST NOT create a persisted version.
*   **Invariant**: Parameter edits MUST NOT mutate persisted state before a validation render succeeds.
*   **Metric**: Number of `commit_generated_version` calls that occur before `render_stl` succeeds (Target: 0).

## 2. Context Isolation
*   **Invariant**: A request MUST use the thread/version snapshot captured at the moment of submission (`snapshotThreadId`, `snapshotWorkingDesign`).
*   **Invariant**: Switching the UI's `activeThreadId` during an in-flight request MUST NOT reroute the result's persistence or overwrite the new active state.
*   **Metric**: Uses of `get(activeThreadId)` inside async pipelines after the initial submission (Target: 0).

## 3. Working-Copy Integrity
*   **Invariant**: `workingCopy.sourceVersionId` MUST always be either a valid persisted message ID or `null`.
*   **Invariant**: Loading history MUST NOT mark the working copy as `dirty`.
*   **Invariant**: Forking MUST clear the persisted lineage (`sourceVersionId`) cleanly.

## 4. Boot Purity
*   **Invariant**: Startup MUST be idempotent. Opening the app twice changes nothing durable.
*   **Invariant**: Boot MAY render a preview but MUST NOT write to persistence unless data is actually repaired/normalized.
*   **Metric**: Number of database writes during `boot()` (Target: 0).

## 5. Module Boundary Discipline
*   **Invariant**: `App.svelte` remains purely compositional. No logic or direct `invoke` calls.
*   **Invariant**: No backend persistence command (`update_parameters`, `commit_version`, etc.) is called from UI components; only stores/controllers.
*   **Invariant**: Prompt context assembly remains backend-owned (`context.rs`).

## 6. Queue Realism
*   **Invariant**: If queue phases exist, they MUST correspond to real scheduling behavior.
*   **Invariant**: Concurrency limits (e.g., `MAX_CONCURRENT_LLM`) must be enforced, not just defined.
*   **Invariant**: Render operations MUST be explicitly serialized (FreeCAD is a singleton).

## 7. Error Fidelity
*   **Invariant**: Raw backend/provider error bodies MUST be preserved and displayed.
*   **Invariant**: No generic fallback messages when a source error is available.

---

## Concrete Metrics to Track
1.  **sessionFlow.ts LOC**: (Current: ~580) - Aim to reduce by splitting.
2.  **Orphaned Invokes**: Direct `invoke(...)` calls outside of `src/lib/stores/` or dedicated controllers.
3.  **Cross-talk instances**: Places where an async pipeline reads global UI state (`activeThreadId`, `activeVersionId`) instead of its own closure-captured snapshot.
4.  **Reachability**: Number of request phases (`RequestPhase`) defined in types vs. actually reachable in E2E tests.
