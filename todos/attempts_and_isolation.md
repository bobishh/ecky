# Feature Plan: Persistent Attempts & Isolation Fix

## 1. The Core Issues
1.  **Ghost Threads (New/Forked):** Currently, when you click "New Thread" or "Fork", no database record is created until the *first generation succeeds*. If you fork and then try to fork again or generate in multiple new threads, the state collapses. Failed attempts or "pending" threads vanish into the void.
2.  **View Bleeding (Model Disappears):** If you are viewing Thread A, and a background request for Thread B transitions from `generating` to `rendering`, Thread A's viewer suddenly clears (model disappears). This means our "isolation" is still leaking UI state updates during background operations.

## 2. Proposed Architecture for "Attempts" (Immutable-like Records)

We will shift to an architecture where **every attempt creates an immediate, permanent record**. There are no "ghosts"; a failed attempt is just as valid a historical artifact as a successful one.

### 2.1 Backend Changes (`src-tauri/src/db.rs` & `commands/generation.rs`)
*   **Immediate Persistence:** When a generation request starts, we immediately write the `Thread` (if it doesn't exist), the User `Message` (the prompt), and a placeholder Assistant `Message` with `status = "pending"` to the database.
*   **Immutable Updates:** 
    *   On success, the pending message is updated with `status = "success"` and the generated `DesignOutput`.
    *   On failure (LLM error or render error), the message is updated with `status = "error"` and the error details.
*   **Failed Threads:** A thread containing only failed attempts is a valid thread. When loaded, it will render `mess.stl` (or a blank state) to reflect its broken reality.

### 2.2 Frontend Changes
*   **History Panel:** 
    *   Display all threads, including those currently pending or fully failed.
    *   Failed threads should be visually distinct but fully selectable.
*   **Request Orchestrator:** 
    *   The `runRequestPipeline` will begin by calling a new `init_generation_attempt` command to secure the DB IDs before the LLM is even invoked.
    *   It will conclude by calling a `finalize_generation_attempt` command with the success/failure payload.
*   **Discarding:** Clicking the "X" on a failed request in the cafeteria simply dismisses the UI notification; the failed attempt remains permanently etched into the thread's history.

## 3. Fixing the Isolation Bug (Model Disappearing)

The bug occurs in `requestOrchestrator.ts` during the `runRequestPipeline` loop:
```javascript
  while (attempt <= req.maxAttempts) {
    if (attempt === 1) {
      session.setStlUrl(null); // <-- BUG: This clears the GLOBAL session url regardless of whether this request belongs to the active thread!
    }
```

### 3.1 The Fix
We must *only* update the global `session` store if the background request actually belongs to the `activeThreadId`.

```javascript
    if (attempt === 1) {
      if (get(activeThreadId) === snapshotThreadId) {
        session.setStlUrl(null);
      }
    }
```
*We must audit the entire `runRequestPipeline` for any other naked calls to `session.setPhase`, `session.setStatus`, `session.setError`, or `session.setStlUrl` and wrap them in an active thread check.*

## 4. Execution Steps

### Phase 1: Fix Isolation & Timeouts (Immediate)
1.  Audit `requestOrchestrator.ts` and wrap all UI-affecting `session.*` calls inside an `if (get(activeThreadId) === req.threadId)` check.
2.  Increase the LLM request timeout (in Rust/LLM client) to 10 minutes to accommodate complex generation tasks.

### Phase 2: Backend Persistence (Immutable Attempts)
1.  Create `init_generation_attempt` command in Rust that creates the thread and inserts the `pending` User/Assistant messages.
2.  Create `finalize_generation_attempt` command in Rust to update the assistant message with the final payload and status (`success` or `error`).
3.  Update `get_thread` and `get_history` to handle threads with no successful outputs (e.g., fallback titles, handle missing macro code).

### Phase 3: Frontend Orchestration
1.  Update `requestOrchestrator.ts` to call the new backend flow.
2.  Ensure "New Thread" and "Fork" correctly generate `snapshotThreadId` so the first request has a target.

### Phase 4: UI Updates (Resend & History)
1.  Update `HistoryPanel` to render threads that have no successful versions yet.
2.  Ensure `loadVersion` handles loading an `error` state message (e.g., forcing `mess.stl`).
3.  Add a "Resend" button to the failed request items in the cafeteria strip (and/or prompt panel) that populates the prompt input with the failed query so the user can easily try again.