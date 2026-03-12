# MCP Phase 1: External Agent Control for FreeCAD Models

## Summary
Phase 1 adds a minimal local `freecad-mcp` server as a second control surface for `ecky`, without changing the existing provider/API-key flow in the desktop app. External agents such as Gemini CLI, Claude Code, and Codex will be able to:

- resolve the current target model/version
- read the current macro, `uiSpec`, and parameters
- patch parameters and rerender a draft
- replace macro code and rerender a draft
- save a successful draft as a new version
- restore an existing saved version

This phase also adds UI-visible markers showing when an external agent is actively modifying a model/version/thread.

Chosen defaults for Phase 1:
- existing API-key flow stays intact
- MCP target selection is `active target by default + explicit override`
- parameter edits use `draft rerender + explicit save`
- agent markers are shown at `thread + version + model` level
- no part-level highlight in this phase

## Product Behavior
The desktop app remains fully functional without MCP. MCP is additive.

When an external agent connects:
- it can operate on the current active model/version if no explicit target is provided
- it can explicitly target a saved version via `messageId`
- it can explicitly target a thread via `threadId`, in which case the server resolves the latest visible successful version in that thread
- it can read the current saved version plus the latest unsaved external draft, if one exists

When an external agent changes parameters or macro code:
- the server performs a rerender
- if rerender succeeds, the server saves a successful unsaved draft
- if rerender fails, the server returns the raw render error and does not overwrite the last successful draft
- the server updates live agent activity markers during the operation

When an external agent explicitly saves:
- the latest successful draft becomes a new saved version in history
- the saved version is created through the same backend validation/runtime path as manual versions
- the external draft for that target is cleared after successful save

When the desktop UI is open:
- thread cards show an external-agent busy badge when that thread is being modified
- version cards show an external-agent busy badge when that saved version is the current target
- the viewer shows external busy status when the current `modelId` matches active external work
- if the local working copy is clean, the UI may hydrate the latest external draft into the current session
- if the local working copy is dirty, the UI must not overwrite local edits and instead show that an external draft changed

## Architecture
### New runtime surface
Add a new standalone binary:
- `src-tauri/src/bin/freecad_mcp.rs`

This binary implements a local `stdio` MCP server. It must not depend on Tauri runtime bootstrapping. It should reuse the same core domain logic and the same persisted storage layout as the desktop app.

### Shared services
Refactor Tauri command internals into shared services so both Tauri commands and `freecad_mcp` call the same logic. The shared layer should own:
- app path/config path resolution
- SQLite connection setup
- target resolution
- render/rerender orchestration
- draft persistence
- external activity persistence
- manual version commit behavior
- restore behavior

The Tauri commands remain as thin wrappers over services.

### Storage model
Use the existing shared database:
- `app_config_dir/history.sqlite`

Keep using:
- saved versions in `threads/messages`
- last session snapshot in `app_config_dir/last_design.json`

Add new tables in `history.sqlite`:
- `agent_sessions`
- `agent_drafts`

Use SQLite WAL as already configured. Do not add a second state file for activity in Phase 1; keep external agent state queryable with the rest of app history/runtime state.

## Data Model
### `agent_sessions`
Purpose: live external activity state for UI markers and target ownership display.

Columns:
- `session_id TEXT PRIMARY KEY`
- `client_kind TEXT NOT NULL`
- `agent_label TEXT NOT NULL`
- `thread_id TEXT`
- `message_id TEXT`
- `model_id TEXT`
- `phase TEXT NOT NULL`
- `status_text TEXT NOT NULL DEFAULT ''`
- `updated_at INTEGER NOT NULL`

Rules:
- `phase` allowed values:
  - `resolving`
  - `reading`
  - `patching_params`
  - `patching_macro`
  - `rendering`
  - `saving_version`
  - `restoring_version`
  - `idle`
  - `error`
- records with `updated_at < now - 30s` are treated as stale and ignored by readers
- session rows are upserted by `session_id`
- session rows are deleted when the MCP client calls shutdown/cleanup if possible, but stale timeout is the correctness mechanism

### `agent_drafts`
Purpose: last successful unsaved external draft per target.

Columns:
- `session_id TEXT NOT NULL`
- `thread_id TEXT NOT NULL`
- `base_message_id TEXT NOT NULL`
- `model_id TEXT`
- `design_output TEXT NOT NULL`
- `artifact_bundle TEXT`
- `model_manifest TEXT`
- `updated_at INTEGER NOT NULL`

Rules:
- one effective latest draft per `(thread_id, base_message_id)` target
- when a new successful draft is written for the same target, it supersedes the previous draft
- failed rerenders do not write draft state
- `version_save` consumes the latest successful draft for the target and then clears it

## Target Resolution
Target resolution must be centralized in one service and used identically by Tauri and MCP.

Input:
- optional `threadId`
- optional `messageId`

Resolution order:
1. if `messageId` is provided:
   - load that saved version
   - error if not found
2. else if `threadId` is provided:
   - resolve the latest visible successful assistant version in that thread
   - error if the thread has no successful version
3. else:
   - read `last_design.json`
   - if it contains a valid `messageId`, resolve that saved version
   - else error with “No active target available.”

Output:
- `threadId`
- `messageId`
- `designOutput`
- `artifactBundle`
- `modelManifest`
- `modelId`
- `latestDraft` if one exists for the target

Phase 1 does not accept `modelId` as an input target selector. It is output-only.

## MCP Tools
### `health_check`
Purpose:
- confirm server is alive and can reach storage/runtime

Input:
- none

Output:
- server version
- database path
- FreeCAD command resolution status
- boolean flags:
  - `dbReady`
  - `freecadConfigured`

Behavior:
- must not mutate state

### `thread_list`
Purpose:
- lightweight browsing of available work targets

Input:
- none

Output for each thread:
- `threadId`
- `title`
- `updatedAt`
- `versionCount`
- `pendingCount`
- `errorCount`

Behavior:
- returns the same visible thread view as current app history

### `thread_get`
Purpose:
- fetch full thread details

Input:
- `threadId`

Output:
- full thread payload with messages/versions
- include `artifactBundle` and `modelManifest` where present

Behavior:
- no mutation

### `target_get`
Purpose:
- fetch the current resolved editable target

Input:
- optional `threadId`
- optional `messageId`

Output:
- resolved target identity
- saved version:
  - `title`
  - `versionName`
  - `macroCode`
  - `uiSpec`
  - `initialParams`
- runtime:
  - `artifactBundle`
  - `modelManifest`
  - `modelId`
- latest successful draft if available:
  - `macroCode`
  - `uiSpec`
  - `initialParams`
  - `artifactBundle`
  - `modelManifest`

Behavior:
- if draft exists, return both `baseVersion` and `draft`
- the client decides whether to continue from draft or from saved base
- update `agent_sessions.phase = reading`

### `params_patch_and_render`
Purpose:
- patch a subset of parameters and rerender a draft

Input:
- optional `threadId`
- optional `messageId`
- `parameterPatch`

Behavior:
- resolve target
- choose source state:
  - latest successful draft if it exists
  - otherwise saved version
- merge patch into current params
- validate merged params against current `uiSpec`
- rerender using the current macro
- on success:
  - write/replace latest successful draft
  - update external activity row
- on failure:
  - return raw error
  - keep previous successful draft unchanged

Output:
- resolved target
- merged params
- resulting `artifactBundle`
- resulting `modelManifest`
- preview path fields from `artifactBundle`
- resulting draft `designOutput`

Rules:
- patch semantics are partial merge, not full replacement
- no saved history version is created here

### `macro_replace_and_render`
Purpose:
- replace macro code and rerender a draft

Input:
- optional `threadId`
- optional `messageId`
- `macroCode`
- optional `parameters`

Behavior:
- resolve target
- choose source state:
  - latest successful draft if it exists
  - otherwise saved version
- if `parameters` omitted, use current params from source state
- run the same macro/controls reconciliation path as manual edit flow
- for framework macros, derive `uiSpec` and params from `CONTROLS`
- for legacy macros, use parsed params if available and reconcile with provided/current params
- rerender
- on success:
  - write/replace latest successful draft
- on failure:
  - return raw error
  - keep previous successful draft unchanged

Output:
- resulting `macroCode`
- resulting `uiSpec`
- resulting `initialParams`
- resulting `artifactBundle`
- resulting `modelManifest`

Rules:
- no saved history version is created here

### `version_save`
Purpose:
- persist the latest successful draft as a new saved version

Input:
- optional `threadId`
- optional `messageId`
- optional `title`
- optional `versionName`

Behavior:
- resolve target
- require an existing successful draft for that target
- save as a new version using the same validation/runtime path as `add_manual_version`
- clear the draft for that target after successful save
- update session state to `saving_version`

Defaults:
- `title`: inherit base version title
- `versionName`: `V-mcp-YYYYMMDD-HHmmss`

Output:
- new `messageId`
- `threadId`
- `modelId`
- saved `designOutput`

### `version_restore`
Purpose:
- restore an existing saved version

Input:
- `messageId`

Behavior:
- call the same restore behavior as desktop history restore
- do not involve drafts
- update session state to `restoring_version`

Output:
- restored target identity

## Desktop App Integration
### Backend read APIs
Add read-only Tauri commands:
- `get_external_agent_sessions`
- `get_external_agent_drafts`

These commands read from the new tables and:
- filter out stale sessions
- optionally filter by `threadId` / `messageId`
- return normalized camelCase contracts to frontend

### Frontend state
Add a new frontend store for external agent state:
- poll every `2s` while app is visible
- poll every `5s` when app is backgrounded if visibility is available
- keep this store separate from `requestQueue`

Add a merged projection layer for UI:
- `threadBusyByExternalAgent(threadId)`
- `versionBusyByExternalAgent(messageId)`
- `modelBusyByExternalAgent(modelId)`
- `latestExternalDraftForTarget(threadId, messageId)`

Do not insert synthetic external requests into `requestQueue`. That queue is local-UI orchestration state; external agent activity is a separate source.

### Draft hydration rules
When external draft changes for the currently active target:
- if local `workingCopy.dirty === false`:
  - hydrate `workingCopy`
  - hydrate `paramPanelState`
  - update session runtime
  - update viewer/model
- if local `workingCopy.dirty === true`:
  - do not hydrate automatically
  - set a visible “agent draft updated” badge/status
  - leave local state untouched

### Viewer/status markers
Show:
- thread badge on history list
- version badge on active/current version cards
- viewer busy banner if current `modelId` has active external work

Displayed text should use:
- `agentLabel`
- `clientKind`
- `phase`
- `statusText`

Phase 1 does not attempt to highlight individual parts.

## File-Level Implementation Outline
Primary new areas:
- `src-tauri/src/bin/freecad_mcp.rs`
- `src-tauri/src/services/`
- `src-tauri/src/mcp/`
- frontend external-agent store and UI projections

Primary existing code to refactor behind services:
- render flow in `src-tauri/src/commands/render.rs`
- manual version/design update flow in `src-tauri/src/commands/design.rs`
- history flow in `src-tauri/src/commands/history.rs`
- last snapshot/session helpers in `src-tauri/src/commands/session.rs`

The implementer should avoid duplicating logic between Tauri and MCP. Shared services are required.

## Error Handling
- Return raw backend/provider/render error bodies where available.
- Do not replace errors with generic “check API key” or “retry later” strings.
- Validation errors for bad parameter patches must identify the offending key and mismatch reason.
- `version_save` without a successful draft must return a clear validation error.
- Target resolution failures must distinguish:
  - unknown message
  - thread has no successful version
  - no active target available

## Testing
### Rust tests
Add tests for:
- target resolution by `messageId`
- target resolution by `threadId`
- target resolution by last snapshot
- no target available
- `params_patch_and_render` partial merge behavior
- invalid parameter patch rejection
- `macro_replace_and_render` on legacy macro
- `macro_replace_and_render` on framework macro
- successful draft superseding previous draft
- failed render preserving previous successful draft
- `version_save` creating new history message from draft
- `version_save` clearing draft after success
- stale session filtering

### Frontend tests
Add tests for:
- thread badge shown for external busy target
- version badge shown for external busy saved version
- viewer busy state shown for matching `modelId`
- clean local working copy auto-hydrates external draft
- dirty local working copy does not auto-hydrate external draft
- external draft updated indicator appears when hydration is blocked

### Verification commands
Before declaring implementation complete:
- `cd /Users/bogdan/Workspace/personal/alcoholics_audacious/ecky/src-tauri && cargo check`
- run relevant Rust tests for new services and DB behaviors
- run relevant frontend tests covering external state UI

## Acceptance Criteria
Phase 1 is complete when:
- a supported MCP host can connect to `freecad_mcp`
- the host can read the current target model/version
- the host can patch parameters and rerender a draft
- the host can replace macro code and rerender a draft
- the host can explicitly save a draft as a new version
- the host can restore a saved version
- the desktop UI visibly marks the thread/version/model currently being changed by an external agent
- local unsaved UI edits are never overwritten by automatic external draft hydration

## Assumptions
- one shared `history.sqlite` remains the source of truth for saved versions
- external drafts are auxiliary transient state, not history
- external clients identify themselves with stable `clientKind` and `agentLabel`
- Phase 1 optimizes for safe local operation, not multi-user remote collaboration
- the implementer should preserve the JS `camelCase` / Rust `snake_case` boundary discipline on all new contracts
