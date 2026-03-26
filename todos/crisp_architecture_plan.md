# Detailed Execution Plan: Attaining a "Crisp" Architecture (v2)

This plan extends the previous architectural targets with a focus on deep linearization of the orchestrator and the "Initial Macro" project startup feature.

## Phase 1: Feedback & Time Decoupling (IN PROGRESS)
*   **Step 1.1: Time Engine** [DONE] - `src/lib/stores/timeEngine.ts` created.
*   **Step 1.2: Phrase Engine** [DONE] - `src/lib/stores/phraseEngine.ts` created.
*   **Step 1.3: Orchestrator Wiring** [DONE] - `requestOrchestrator.ts` uses `phraseEngine`.
*   **Step 1.4: App.svelte Cleanup** [DONE] - `App.svelte` state reduced.

## Phase 2: Orchestrator Linearization & Class-Based State Machine

The goal is to turn `runRequestPipeline` from a procedural monolith into a declarative state machine.

### Step 2.1: The `PipelineTask` abstraction
Instead of a single `GenerationPipeline` class, we will define a `PipelineTask` interface.
*   Each major step (Classify, Generate, Render, Commit) becomes a class implementing `PipelineTask`.
*   A `PipelineController` manages the transition between tasks.

### Step 2.2: Formalizing "Attempts" as First-Class Citizens
*   Every attempt will have a dedicated `AttemptRecord` in the DB (already partially done).
*   The orchestrator will maintain an `AttemptChain` allowing for branching or re-runs of specific steps without restarting the entire pipeline.

## Phase 3: Project Startup with Initial Macro (New Feature)

The user needs to be able to start a project with existing code.

### Step 3.1: UI - "New Project" Modal
*   Update `HistoryPanel` to show a modal when "➕" is clicked.
*   Modal Options:
    *   "Blank Session" (current behavior).
    *   "Import Macro" (Textarea or File Upload).
*   State: Captured in a new `NewThreadWizard.svelte` component.

### Step 3.2: Persistence - `commit_manual_version` on New Thread
*   Update `manualController.ts` to support creating a thread *from a version commit* if no `activeThreadId` exists.
*   Ensure the first message in the thread is the imported macro, marked as a "Manual Import".

### Step 3.3: Integration with Orchestrator
*   The `GenerationPipeline` must be aware of the "Initial Design" state.
*   If a thread starts with a manual macro, subsequent LLM requests must use that macro as the `parentMacroCode`.

## Phase 4: Real Task Scheduling & Concurrency Control

*   **Render Serialization**: FreeCAD is a singleton process. We must implement a `Mutex` or a serial queue in `scheduler.ts` to ensure only one render happens at a time across all threads.
*   **LLM Concurrency**: Limit concurrent API calls to prevent flooding or hitting provider rate limits.

---

## Targeted "Crisp" Improvements for Request Lifecycle

1.  **Immutability**: Once an attempt starts, its input prompt and snapshots are frozen.
2.  **Recoverability**: If the app crashes mid-generation, the `pending` status in DB allows the UI to offer a "Resume" or "Cleanup" action on boot.
3.  **Auditability**: Every FreeCAD traceback is stored in the `error_message` field of the attempt message, viewable in the history.
