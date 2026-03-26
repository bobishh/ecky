# Comprehensive Improvement Plan — B/B+ Areas

## Table of Contents
1. [Test Coverage (B → A−)](#1-test-coverage-b--a)
2. [State Management (B+ → A)](#2-state-management-b--a)
3. [Code Conventions (B+ → A)](#3-code-conventions-b--a)
4. [Documentation (B → A−)](#4-documentation-b--a)
5. [Security (B → A−)](#5-security-b--a)
6. [Agent Ecosystem (B+ → A)](#6-agent-ecosystem-b--a)
7. [Import Story (B+ → A−)](#7-import-story-b--a)
8. [Polish / Edge Cases (B → A−)](#8-polish--edge-cases-b--a)

---

## 1. Test Coverage (B → A−)

**Current state:** 79 Rust unit tests, 10 Playwright E2E specs, 2 TS unit tests (`followUpGuard.test.ts`, `traits.test.ts`). Controllers and stores are completely untested.

### Phase 1 — Controller Unit Tests (High Impact)

**File: `src/lib/controllers/requestOrchestrator.test.ts`**

The `GenerationPipeline` class is the most critical untested code. Test:
- `isExplicitQuestionOnlyIntent()` — 15+ localized markers including Russian
- `isQuestionIntent()` — question vs. design heuristic
- `isGenericRoutingResponse()` — classifier bubble filtering
- `mergeUsageSummary()` — segment aggregation arithmetic
- `requestSignature()` / `findRecentDuplicateRequest()` — dedup within 1500ms window
- `pickRetryMessage()` — doesn't crash, returns string with attempt count

These are all pure functions extractable without mocking Tauri.

**File: `src/lib/controllers/manualController.test.ts`**

Test:
- `mergeFieldWithExisting()` — preserves user label/frozen/bounds over parsed
- `coerceParamValue()` — type coercion for each field type
- `fallbackParamValue()` — correct defaults per type
- `reconcileManualControls()` — requires mocking `parseMacroParams`, but the merge logic is testable standalone

### Phase 2 — Store Unit Tests (Medium Impact)

**File: `src/lib/stores/requestQueue.test.ts`**

The request queue has a real state machine. Test:
- `submit()` — creates request, sets active
- `patch()` — merges, computes `cookingElapsed` on terminal phase
- `cancel()` — only works for non-terminal phases
- `remove()` — shifts `activeId` when removing active request
- Derived stores: `activeThreadBusy`, `activeThreadModelBusy`, `llmInFlightCount`

**File: `src/lib/stores/workingCopy.test.ts`**

- `loadVersion()` → normalizes and sets `dirty: false`
- `patch()` → auto-sets `dirty: true`
- `updateParams()` → merges, not replaces
- `reset()` → returns to initial state

**File: `src/lib/stores/paramPanelState.test.ts`**

- `hydrateFromVersion()` → extracts uiSpec/params from DesignOutput
- `patchParams()` → merges partial params

### Phase 3 — Model Runtime Unit Tests (Medium Impact)

**File: `src/lib/modelRuntime/semanticControls.test.ts`**

- `ensureSemanticManifest()` — auto-generates control primitives/views from UI spec
- `materializeControlViews()` — resolves bindings into rendered control data
- `buildSemanticPatch()` — builds param deltas from primitive value changes
- `resolveActiveControlViewId()` — part-scoping logic

**File: `src/lib/modelRuntime/importedRuntime.test.ts`**

- `buildImportedUiSpec()` — generates UI fields from manifest parts
- `buildImportedParams()` — merges manifest defaults with active params
- `buildImportedPreviewTransforms()` — computes scale/translate for preview

### Phase 4 — Rust Backend Gap Coverage

Add tests for:
- `src-tauri/src/commands/generation.rs` — `prepare_images()`, `selected_engine()` validation
- `src-tauri/src/services/target.rs` — lease acquisition/expiry logic
- `src-tauri/src/mcp/server.rs` — JSON-RPC dispatch (integration test with mock Axum state)

### Infrastructure

- Add a `test:unit:watch` script: `"tsx watch --test src/lib/**/*.test.ts"`
- Add CI step: `npm run test:unit && cd src-tauri && cargo test`
- Target: **30+ TS unit tests** covering the pure logic in controllers, stores, and modelRuntime

---

## 2. State Management (B+ → A)

**Current state:** Good store decomposition, but `App.svelte` is a 1,379-line god component wiring everything together. `sessionStore.ts` uses `any` for `agentDraft`.

### 2.1 — Extract Viewport Overlay Module

Create `src/lib/ViewportOverlayManager.svelte`:
- Extract the error banner, agent activity banner, agent draft toast, agent confirm stack, cafeteria strip from `App.svelte`
- Props: `error`, `activeAgentSessions`, `agentDraft`, `viewportRequests`, `pendingConfirms`
- Events: `onDismissError`, `onApplyDraft`, `onRejectDraft`, `onConfirm`

### 2.2 — Extract Cafeteria Strip

Create `src/lib/CafeteriaStrip.svelte`:
- The "microwave units" rendering (CSS + `{#each}` rendering logic)
- Props: `requests: Request[]`, `nowSeconds`, `onCancel`, `onRemove`, `onClick`
- Self-contained visual component with no business logic

### 2.3 — Extract Resizer Logic

Create `src/lib/utils/resizer.ts`:
- The `handleSidebarResize`, `handleDialogueResize` functions and their pointer-event wiring
- Export as a reusable `createResizer(axis, store, min, max)` utility
- Currently duplicated for sidebar width, history height, and dialogue height

### 2.4 — Type the Agent Draft

In `sessionStore.ts`, replace:
```ts
agentDraft: null as any | null
```
with:
```ts
agentDraft: null as AgentDraft | null
```
Import `AgentDraft` from `../types/domain`.

### 2.5 — Extract Agent Session Poller

The agent session polling logic in `App.svelte` (the `setInterval` that calls `getActiveAgentSessions`) should become a standalone reactive module:
- `src/lib/stores/agentSessionPoller.ts`
- Exports a writable store `activeAgentSessions` and `startPolling()`/`stopPolling()` lifecycle functions
- Encapsulates the poll interval, error handling, and MCP status refresh

**Target:** `App.svelte` drops from 1,379 lines to ~600–700 lines.

---

## 3. Code Conventions (B+ → A)

### 3.1 — Clean Root Directory

| File | Action |
|---|---|
| `fix_params.cjs` | Delete (one-shot migration, in git history) |
| `patch_gen_rs.js` | Delete |
| `patch_lib_rs.cjs` | Delete |
| `patch_parampanel.js` | Delete |
| `run_qa_test.js` | Move to `scripts/run_qa_test.js` or delete |

### 3.2 — Relocate `server/freecad_runner.py`

`freecad_runner.py` is invoked as a subprocess by `src-tauri/src/freecad.rs` via `RUNNER_RESOURCE_PATH = "server/freecad_runner.py"`. It has nothing to do with the Express server.

- Move to `model-runtime/freecad_runner.py` (alongside `cad_sdk.py`)
- Update `RUNNER_RESOURCE_PATH` in `freecad.rs`
- Update `tauri.conf.json` resource mapping if needed

### 3.3 — Clarify or Remove the Express Server

`server/index.ts` and `server/prompt.ts` appear to be a standalone Express dev server (non-Tauri mode). Decide:
- **Keep:** Rename directory to `server-standalone/`, add a README explaining it's for web-only dev
- **Remove:** If it's unused, delete entirely. The Tauri backend handles all routing now.

### 3.4 — CSS Duplication

Several components duplicate identical styles (`.btn`, `.btn-secondary`, `.btn-danger`, `.confirm-delete-body`, `.confirm-actions`). These appear verbatim in `PromptPanel.svelte`, `HistoryPanel.svelte`, and `ParamPanel.svelte`.

- Extract shared button styles into `src/styles/shared.css`
- Import via Svelte's `@import` or global styles in `src/styles/app.css`

---

## 4. Documentation (B → A−)

### 4.1 — Rust API Doc Comments

Add `///` doc comments to all public functions in:
- `src-tauri/src/freecad.rs` — `render()`, `render_model()`, `import_fcstd()`, `apply_imported_model()`
- `src-tauri/src/llm.rs` — `generate_design()`, `classify_intent()`, `list_models()`
- `src-tauri/src/db.rs` — `init_db()`, `get_all_threads()`, `add_message()`, `delete_version_cluster()`
- `src-tauri/src/contracts.rs` — key types (`AppError`, `DesignOutput`, `ArtifactBundle`, `ModelManifest`)

Format: one-line summary + parameter notes for non-obvious args. Example:
```rust
/// Renders a FreeCAD macro with the given parameters and returns the artifact bundle.
///
/// Uses content-hash caching — identical (macroCode, params) pairs return cached results.
/// The render lock ensures only one FreeCAD subprocess runs at a time.
pub fn render_model(...) -> AppResult<ArtifactBundle> {
```

### 4.2 — Architecture Decision Record

Create `docs/ARCHITECTURE.md`:
- Tauri boundary contract (camelCase ↔ snake_case via serde)
- Content-hash caching strategy in `freecad.rs`
- Request queue state machine (phases, transitions)
- MCP server session lifecycle (initialize → acquire lease → tool calls → finalize)
- Model manifest schema (parts, control primitives, control views, enrichment proposals)

### 4.3 — MCP Integration Guide

Create `docs/MCP.md`:
- Available tools (with argument schemas)
- Session lifecycle diagram
- Target lease semantics
- Confirmation flow
- Config snippets (already exist in ConfigPanel — extract to doc)

---

## 5. Security (B → A−)

### 5.1 — OS Keychain for API Keys

Replace plaintext `config.json` storage with OS-native credential storage:

**Rust side:**
- Add `keyring = "3"` to `Cargo.toml`
- New `src-tauri/src/services/secrets.rs`:
  - `store_api_key(engine_id: &str, api_key: &str) -> AppResult<()>`
  - `get_api_key(engine_id: &str) -> AppResult<String>`
  - `delete_api_key(engine_id: &str) -> AppResult<()>`
- On `save_config`, extract `api_key` from each engine, store in keyring, write `"api_key": ""` to disk
- On `get_config`, hydrate `api_key` from keyring before returning to frontend
- Fallback: if keyring unavailable (e.g., headless Linux), fall back to current file-based approach with a warning

**Frontend side:**
- No changes needed — the API key field in ConfigPanel continues to work as-is
- On save, the backend handles the split

### 5.2 — Redact API Keys from Logs

Audit all `eprintln!` and `console.error` calls in:
- `src-tauri/src/llm.rs` — ensure `api_key` is never included in error messages
- `src-tauri/src/mcp/server.rs` — ensure request/response logging doesn't leak keys

---

## 6. Agent Ecosystem (B+ → A)

**Current state:** Full MCP server with tool definitions, session lifecycle, target leases, and draft/confirm flow. But the tool surface is narrow and there's no observability.

### 6.1 — Expand MCP Tool Surface

Add these tools to `src-tauri/src/mcp/handlers.rs`:

| Tool | Description | Priority |
|---|---|---|
| `get_thread_history` | List all threads with summaries (agents currently can't browse) | High |
| `read_macro_code` | Read the current macro source for a thread/version | High |
| `patch_macro` | Modify specific lines of a macro (avoid full regen) | Medium |
| `export_model` | Export current model as STEP/STL to a specified path | Medium |
| `get_usage_summary` | Token/cost summary for the current session | Low |
| `list_parameters` | Return current param keys, types, values, and bounds | Medium |

### 6.2 — Agent Session Observability

Create `src-tauri/src/mcp/metrics.rs`:
- Track per-session: tool call count, total latency, lease renewals, errors
- Expose via a new MCP resource: `ecky://metrics/session/{session_id}`
- Emit Tauri event `mcp:session-metrics` for real-time UI display

### 6.3 — Agent Status Bar Enhancement

In `src/lib/AgentStatusBar.svelte`:
- Show connected agent count, active leases, and aggregate phase
- Add a "disconnect all" button
- Show per-agent LLM model and current tool call

### 6.4 — MCP Authentication

Add optional bearer token auth:
- New config field: `mcp.authToken: string | null`
- Server validates `Authorization: Bearer <token>` header
- Without a token, server accepts all local connections (current behavior)

---

## 7. Import Story (B+ → A−)

**Current state:** FCStd import → runner report → heuristic proposals from bounding box → user manually accepts/rejects in ParamPanel. The UX gap is between "proposals exist" and "user understands what they mean."

### 7.1 — Guided Enrichment Wizard ✅ DONE

Created `src/lib/ImportEnrichmentModal.svelte`:
- Shown automatically after FCStd import when proposals are `pending` (wired in `App.svelte`)
- Shows part count, proposal cards with accept/reject toggle (click-to-toggle UX)
- Bulk "ACCEPT ALL" / "REJECT ALL" buttons
- Commits manifest via `saveModelManifest`, updates history store, refreshes session state
- Summary chips show accepted/rejected/total counts in real-time

### 7.2 — Live Preview During Enrichment ✅ DONE

- Hovering over a proposal card in the wizard calls `onSelectPart(proposal.partIds[0])` 
- This highlights the corresponding part in the 3D viewer (uses existing `selectedPartId` / `handlePartSelect` flow)
- Mouse leave clears the selection

### 7.3 — Enrichment Status Indicator ✅ DONE

- Added `hasImportPendingSetup(thread)` function to `HistoryPanel.svelte`
- Renders "NEEDS SETUP" badge (pulsing bronze accent) on thread cards with pending imported model proposals
- Uses existing `status-pulse` animation keyframes

### 7.4 — Re-enrichment After Re-render

When an imported model is re-rendered with new params (`apply_imported_model`):
- If new objects appear in the runner report that weren't in the original manifest, auto-generate new proposals
- Show a toast: "New parts detected — review parameter bindings?"

**Status: 3/4 complete. 7.4 deferred — requires Rust backend changes to detect new objects during re-render.**

---

## 8. Polish / Edge Cases (B → A−)

### 8.1 — Undo for Parameter Changes

Create `src/lib/stores/paramHistory.ts`:
- Circular buffer of last 20 `{ params, uiSpec }` snapshots
- `pushSnapshot()` called before every `handleParamChange`
- `undo()` restores previous snapshot and triggers re-render
- Wire Ctrl+Z / Cmd+Z keyboard shortcut in `ParamPanel.svelte`

### 8.2 — Prompt Draft Robustness

In `PromptPanel.svelte`, the draft persistence uses `localStorage` which can silently fail:
- Add a `try/catch` around `JSON.parse` (already done ✓)
- Add max-size guard: if drafts exceed 500KB total, evict oldest entries
- Clear draft for a thread when the thread is deleted

### 8.3 — Export Button in Viewport

Add a prominent "Export" button to the viewport overlay (bottom-right):
- Options: STL, STEP, FCStd
- Calls `exportFile(format, path)` via the Tauri client
- Uses `save` dialog from `@tauri-apps/plugin-dialog` for path selection
- The `exportFile` function already exists in `client.ts` — it just needs UI surface

### 8.4 — Keyboard Shortcuts

Add global keyboard shortcuts (wire in `App.svelte` or a dedicated `src/lib/utils/shortcuts.ts`):

| Shortcut | Action |
|---|---|
| `Ctrl+Enter` | Submit prompt (already works in textarea) |
| `Ctrl+E` | Toggle code editor |
| `Ctrl+Z` | Undo parameter change (see 8.1) |
| `Ctrl+Shift+Z` | Redo parameter change |
| `Ctrl+S` | Save current parameter values |
| `Escape` | Close modal / dismiss error |
| `[` / `]` | Previous / next version |

### 8.5 — Graceful Degradation When FreeCAD Missing

Currently, if `freecadcmd` is not found, the user gets a raw subprocess error. Instead:
- On boot, check if `freecadcmd` is available (run `freecadcmd --version`)
- If missing, show a persistent warning banner with install instructions per platform
- Disable the "Generate" button with tooltip: "FreeCAD not found — configure in Settings"

### 8.6 — Request Queue Memory Management

The cafeteria strip keeps all completed requests in memory (including base64 screenshots). Add:
- Auto-eviction of terminal requests older than 5 minutes
- Cap at 20 total requests in the queue
- Clear screenshot data from terminal requests immediately (keep only the result)

---

## Priority Matrix

| Item | Effort | Impact | Do First? |
|---|---|---|---|
| 1. Phase 1 — Controller tests | S | High | ✅ |
| 2.1–2.2 — Extract overlay + cafeteria | M | High | ✅ |
| 3.1 — Clean root dir | XS | Medium | ✅ |
| 8.3 — Export button | S | High | ✅ |
| 8.5 — FreeCAD missing check | S | High | ✅ |
| 2.4 — Type agent draft | XS | Low | ✅ |
| 4.1 — Rust doc comments | M | Medium | |
| 1. Phase 2 — Store tests | M | Medium | |
| 5.1 — Keychain storage | M | Medium | |
| 7.1 — Enrichment wizard | L | High | ✅ DONE |
| 6.1 — Expand MCP tools | L | Medium | |
| 3.4 — CSS dedup | S | Low | |
| 4.2–4.3 — Architecture docs | M | Medium | |
| 8.1 — Param undo | M | Medium | |
| 8.4 — Keyboard shortcuts | S | Medium | |
| 6.4 — MCP auth | S | Low | |
| 8.6 — Queue memory mgmt | S | Low | |
