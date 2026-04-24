# Frontend Architecture Review & Split Plan

**Date:** 2026-04-20
**Status:** Plan doc. Current progress below.

## Status Update — 2026-04-23

### Phase 1 — App extraction

Mostly landed.

- Extracted from `App.svelte`:
  - `dialogueState.ts`
  - `viewerBusyState.ts`
  - `viewportState.ts`
  - `contextState.ts`
  - `agentOps.ts`
  - `exportOps.ts`
- Browser proof green:
  - `e2e/runtime-capabilities.spec.ts`
  - `e2e/dialogue-mcp-thread.spec.ts`

### Phase 2 — ParamPanel split

In progress. Views path now split into focused components.

- Extracted:
  - `ParamPanelControlField.svelte`
  - `ParamPanelAdvisoryList.svelte`
  - `ParamPanelContextStrip.svelte`
  - `ParamPanelPrimitiveComposer.svelte`
  - `ParamPanelAdvisoryComposer.svelte`
  - `ParamPanelRelationComposer.svelte`
  - `ParamPanelViewComposer.svelte`
- Browser proof green:
  - `e2e/params.spec.ts`

### Remaining frontend debt

- `ParamPanel.svelte` still owns:
  - semantic section renderer
  - relation chip list
  - raw/focused control slices
  - measurement/lithophane-heavy branches
- `App.svelte` still large, but no longer sole home for dialogue/context/export/viewer-busy logic.

---

## 1. Inventory

| Area | Files | LoC | Role |
|------|-------|-----|------|
| **App.svelte** | 1 | **4,002** | God component — all state, orchestration, layout |
| **Panel components** | 15 `.svelte` | 15,644 | ParamPanel (4466), Viewer (2130), ConfigPanel (2055), PromptPanel (1571), HistoryPanel (919), VertexGenie (719), ProjectSwitcher (682) |
| **Stores** | 12 `.ts` | 2,339 | domainState, history, requestQueue, sessionStore, windowStore, paramPanelState, etc. |
| **Controllers** | 5 `.ts` | 2,603 | requestOrchestrator (1071), manualController (547), verificationLoop (226), structuralVerification (96), followUpGuard (118) |
| **Tauri client** | 2 `.ts` | 1,551 | contracts.ts (generated), client.ts (694 — typed invoke wrappers) |
| **Agents** | 5 `.ts` | 1,839 | state, activity, terminal, screenshot, workspaceCapture |
| **Model Runtime** | 6 `.ts` | 2,996 | semanticControls, contextualEditing, runtimeBundle, importedRuntime, sessionSnapshot |
| **Types** | 2 `.ts` | 1,420 | domain.ts (1268), phase.ts |
| **Other** | 7 `.ts` | 2,821 | genie/traits, boot/restore, audio/microwave, viewportBlueprint, exportOptions, etc. |
| **Styles** | 2 `.css` | 1,335 | app.css (1178), style.css (157) |
| **Total frontend** | | **~36,600** | |

---

## 2. Architecture As-Is

```
main.ts
  └─ App.svelte (4002 LoC god component)
       ├── 54 imports at top
       ├── ~2700 lines of <script> (state, derived, functions, lifecycle)
       ├── ~800 lines of template (layout, conditionals, event wiring)
       ├── ~500 lines of <style>
       │
       ├── Viewer.svelte (Three.js viewport, 2130 LoC)
       ├── PromptPanel.svelte (chat input, 1571 LoC)
       ├── ParamPanel.svelte (parameter controls, 4466 LoC — biggest component)
       ├── HistoryPanel.svelte (thread/version browser, 919 LoC)
       ├── ConfigPanel.svelte (settings, 2055 LoC)
       ├── ProjectSwitcher.svelte (thread nav, 682 LoC)
       ├── VertexGenie.svelte (mascot/status, 719 LoC)
       ├── DrawingOverlay.svelte (sketch overlay, 320 LoC)
       ├── Window.svelte (draggable window shell, 248 LoC)
       ├── CodeModal.svelte (code viewer, 183 LoC)
       ├── AgentTerminalSurface.svelte (xterm, 236 LoC)
       └── ...modals
```

### Data flow

```
Tauri Backend (Rust)
    ↕  invoke() / listen()
tauri/client.ts (typed wrappers, camelCase translation)
    ↕
stores/ (Svelte writable/derived stores)
    ↕
controllers/ (orchestration logic, stateful workflows)
    ↕
App.svelte (wires everything together)
    ↕
Panel components (consume stores + callbacks from App)
```

---

## 3. What Works Well

### 3.1 Clean Tauri boundary
`tauri/client.ts` provides typed wrappers over generated `contracts.ts`. camelCase on JS side, snake_case on Rust. All normalization (field renaming, legacy healing) happens in one place.

### 3.2 Store architecture is solid
Stores are well-separated by domain: `domainState` (canonical), `requestQueue` (in-flight work), `sessionStore` (UI session), `windowStore` (layout), `paramPanelState` (live-apply). Derived stores compute what the UI needs without side effects.

### 3.3 Controller separation
`requestOrchestrator` handles the full generate→render→verify loop. `manualController` handles param changes and manual commits. `verificationLoop` is cleanly extracted. Pure business logic, testable without Svelte.

### 3.4 Model Runtime utilities
`semanticControls`, `contextualEditing`, `runtimeBundle` are pure TS modules with test coverage. No Svelte dependency — they transform data.

### 3.5 Agent subsystem is modular
`agents/` is cleanly scoped: `state.ts` (derivations), `activity.ts` (phase mapping), `terminal.ts` (key/line input), `screenshot.ts` (viewport capture), `workspaceCapture.ts` (workspace images). All have tests.

### 3.6 Windowing system
`windowStore.ts` with per-thread layout persistence, draggable `Window.svelte` shell. Desktop-native feel with z-ordering, minimize, per-window mount policies.

---

## 4. Issues Found

### 4.1 — CRITICAL: `App.svelte` is a 4,002-line god component

54 imports, ~120 `$state`/`$derived` declarations, ~80 functions, all in one file. This is the single biggest structural debt. It handles:

- Thread/version navigation state
- Agent dialogue state machine
- Viewport capture & screenshot resolution
- Concept preview / blueprint viewport state
- Window layout orchestration
- Export flow
- Error banner
- Onboarding overlay
- Audio (microwave hum) lifecycle
- Param live-apply coordination
- Semantic control view resolution

**Impact:** Any feature change touches this file. Merge conflicts are guaranteed. Reasoning about state interactions requires reading thousands of lines.

### 4.2 — CRITICAL: `ParamPanel.svelte` at 4,466 lines

The largest component in the codebase. Handles:
- All parameter control types (number, select, checkbox, image)
- Semantic view switching
- Context target selection
- Measurement annotations
- Control primitive editing
- Advisory display
- Live-apply preview

This is essentially a sub-application crammed into one component.

### 4.3 — MODERATE: Template logic is business logic

`App.svelte` template has complex conditional chains (`{#if dialogueState.mode === 'agent-reply'}`, `{#if showBlueprintViewport && effectiveConceptPreviewMessage}`) that encode workflow rules. These are untestable — they only execute in the browser.

### 4.4 — MODERATE: Event handlers defined in App, passed as callbacks

Panel components receive event handlers from App via props. Example: `ParamPanel` gets `onParamChange`, `onCommit`, `onFork`, etc. This creates a callback-threading problem where App is the only place that can wire components together.

### 4.5 — MODERATE: Derived state chains are implicit

`effectiveUiSpec` depends on `$paramPanelState.uiSpec` and `sessionModelManifest`. `activeModelManifest` depends on `effectiveUiSpec`, `effectiveParameters`, and `sessionModelManifest`. These chains live in App's `<script>` and are invisible to the type system.

### 4.6 — MINOR: CSS is ~500 lines in App + 1178 in global

Most component CSS is scoped (good), but App's `<style>` block contains layout classes for the cafeteria strip, blueprint viewport, error banner, boot overlay, and agent terminal. These should live with their components.

### 4.7 — MINOR: No router

Single-page app with no routing. All view switching (`currentView`, `showCodeModal`, window visibility) is manual state flags. Acceptable for a desktop app, but makes deep-linking or multi-window future harder.

---

## 5. Split Plan

### Guiding principle

This is a **desktop CAD app**, not a web SPA. There is one viewport, one active project, one parameter panel. The right decomposition axis is **concern boundaries**, not page routes.

### Phase 1: Extract App.svelte state machines into composables (2-3 days)

Extract coherent state groups from App's `<script>` into Svelte-5 compatible reactive modules (`*.svelte.ts`). No component splitting yet — just move logic.

| New module | Lines moved | What |
|------------|------------|------|
| `lib/composables/dialogueState.svelte.ts` | ~100 | `DialogueState` type, `dialogueState` derivation, `handleDialogueSubmit`, agent prompt resolution |
| `lib/composables/viewportState.svelte.ts` | ~200 | `viewerBusyPhase`, `hasRenderableModel`, `viewerAssets`, `stlUrl`, concept preview state, blueprint viewport logic |
| `lib/composables/contextState.svelte.ts` | ~150 | `effectiveUiSpec`, `effectiveParameters`, `activeModelManifest`, `contextSelectionTargets`, control view resolution |
| `lib/composables/agentOps.svelte.ts` | ~250 | Agent session polling, agent state refresh, viewport screenshot handling, terminal wiring |
| `lib/composables/exportOps.svelte.ts` | ~80 | Export modal state, export handlers |

**Result:** App.svelte drops from 4002 to ~2200 lines. The extracted modules are testable.

### Phase 2: Split ParamPanel (2-3 days)

| New component | What |
|---------------|------|
| `ParamControls.svelte` | Number/select/checkbox/image control rendering |
| `SemanticViewSwitcher.svelte` | View tabs, target selector |
| `MeasurementPanel.svelte` | Measurement annotations display/edit |
| `ControlPrimitiveEditor.svelte` | Semantic knob create/edit |
| `AdvisoryList.svelte` | Advisory display |

`ParamPanel.svelte` becomes a layout shell that composes these.

### Phase 3: Extract viewport overlays from App template (1-2 days)

| New component | What |
|---------------|------|
| `CafeteriaStrip.svelte` | The multi-microwave request strip (currently ~120 lines of template in App) |
| `BlueprintViewport.svelte` | Concept preview overlay (currently ~80 lines in App) |
| `ErrorBanner.svelte` | Error display strip (currently ~50 lines in App) |
| `BootOverlay.svelte` | Boot loading screen (currently ~15 lines in App) |
| `ViewportOverlayControls.svelte` | Outline/topology/export controls overlay |

### Phase 4: CSS extraction (1 day)

Move App's `<style>` blocks into the components that own them (CafeteriaStrip, BlueprintViewport, etc.). App.svelte retains only layout grid CSS.

### Phase 5: Type tightening (ongoing)

Replace callback threading with typed Svelte context or event dispatchers:

```typescript
// Before: App passes 8 callbacks to ParamPanel
<ParamPanel 
  onParamChange={handleParamChange}
  onCommit={commitManualVersion}
  ...
/>

// After: ParamPanel dispatches typed events
<ParamPanel on:paramChange on:commit ... />
```

---

## 6. Backend Feature Matrix (Rust side)

| Area | LoC | Health |
|------|-----|--------|
| `mcp/` (handlers, runtime, server, contracts) | ~12,800 | Largest subsystem. Well-structured with clear handler dispatch. |
| `ecky_scheme/` (compiler, bootstrap, cad, params, core) | ~6,200 | Solid. Two compilation paths (expanded AST + full runtime). |
| `ecky_ir/` (lowering, model, mesh_ops, runtime, sketch, edge_ops) | ~10,500 | Core engine. Reviewed in detail — see build123d-lowering-review.md. |
| `commands/` (session, design, generation, render) | ~5,500 | Clean Tauri command boundary. |
| `services/` (render, history, agent_versions, structural_verification) | ~3,200 | Well-separated service layer. |
| `db.rs` | 2,970 | SQLite persistence. Single file but scoped queries. |
| `llm.rs` + `llm_context.rs` | 2,426 | LLM client + context building. |
| `contracts.rs` | 4,152 | Shared types. Auto-generates TS contracts. |

---

## 7. Verdict

**The frontend is functionally complete and well-tested (128 unit tests, all passing).** The store/controller/client layer is well-architected. The Tauri boundary is clean.

**The structural debt is concentrated in two files:**
- `App.svelte` (4002 LoC god component) — should be decomposed into composables + sub-components
- `ParamPanel.svelte` (4466 LoC) — should be split into focused sub-components

**The split plan preserves the desktop app's single-viewport paradigm.** No routing needed. The decomposition axis is state-machine boundaries (dialogue, viewport, context, agent, export), not pages. Each phase is independently shippable and backward-compatible.

**Estimated total effort:** ~8-12 days for Phases 1-4. Phase 5 is ongoing hygiene.
