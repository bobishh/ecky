# Plan: MCP UI Interaction & Parameter Highlighting

This plan addresses the idea of giving the MCP Agent (Gemini) the ability to physically control the Ecky app UI, specifically opening windows and highlighting parameters during changes.

## Goal
Allow the agent to execute actions like "show & change & render" or simply "show". This gives the human user immediate visual feedback in the UI about what the agent is modifying, creating a more collaborative and integrated feel.

## Phase 1: Backend Infrastructure (Rust / Tauri)
We need a way for the MCP server (running in the Rust backend) to send events to the Svelte frontend.

1. **New Tauri Event:**
   Implement a global event system in `src-tauri/src/mcp/server.rs` or `main.rs`.
   Example Event Name: `mcp://ui-dispatch`
   Payload: 
   ```json
   {
     "action": "openWindow" | "highlightParam" | "closeWindow",
     "target": "params", // or specific param key like "wall_thickness"
     "value": "optional new value to preview"
   }
   ```

2. **New MCP Tool (`mcp_ecky_mcp_ui_dispatch`):**
   Add a new tool to the MCP protocol.
   - **Description:** "Trigger a UI action in the Ecky frontend to show the user what you are doing (e.g. open the parameters window, highlight a specific slider)."
   - **Input Schema:** Matches the event payload above.

3. **Enhance `mcp_ecky_mcp_params_preview_render`:**
   Modify the existing parameter patching tool to automatically emit an `openWindow` event for "params" and a `highlightParam` event for every key being patched, *just before* it triggers the re-render. This gives a "Show -> Change -> Render" flow automatically.

## Phase 2: Frontend Implementation (Svelte)
The frontend needs to listen for these events and react visually.

1. **Window Store (`src/lib/stores/windowStore.ts`):**
   - Add a listener for `mcp://ui-dispatch` events.
   - If `action == "openWindow"`, call `toggleWindow(target, true)` and `bringToFront(target)`.

2. **Parameter Panel (`src/lib/ParamPanel.svelte`):**
   - Subscribe to the `mcp://ui-dispatch` event.
   - If `action == "highlightParam"`, find the DOM element corresponding to the parameter `key`.
   - Scroll the element into view: `element.scrollIntoView({ behavior: 'smooth', block: 'center' })`.
   - Add a temporary CSS class (e.g., `.highlight-pulse`) to the parameter row.

3. **CSS Animation:**
   Add a keyframe animation in `app.css` or `ParamPanel.svelte`:
   ```css
   @keyframes highlightPulse {
     0% { background-color: transparent; }
     50% { background-color: var(--primary); color: var(--bg-100); }
     100% { background-color: transparent; }
   }
   .highlight-pulse {
     animation: highlightPulse 2s ease-in-out;
   }
   ```

## Workflow Example
User: "Make the wall thicker."
Agent:
1. `ui_dispatch({ action: "openWindow", target: "params" })`
2. `ui_dispatch({ action: "highlightParam", target: "wall_thickness" })`
3. `params_preview_render({ "wall_thickness": 3.0 })`

## Next Steps
- Implement the Tauri event emitter in the MCP server.
- Wire up the event listener in Svelte `App.svelte` or a global layout.
- Test the CSS animation on sliders.

## Agent Validation Feedback Bubble Plan

### Goal

Show the feedback loop the model already sees while draft preview evolves.

User-visible flow:

1. Agent edits source or params.
2. Preview updates in viewport.
3. Ecky bubble shows compact validation / repair feedback tied to that draft.
4. Raw terminal/error detail stays in Agent Terminal, not app logs or dialogue spam.

This turns "brick becomes cylinder" preview watching into "brick becomes cylinder because validation says X/Y/Z".

### Current Code Anchors

- Draft preview event already exists: `src-tauri/src/contracts.rs` `AgentDraftPreviewUpdatedEvent`.
- Backend emits draft preview: `src-tauri/src/models.rs` `emit_agent_draft_preview_updated`.
- Preview store path: `src-tauri/src/mcp/handlers.rs` `store_session_render_preview`.
- Frontend consumes preview event: `src/App.svelte` `agent-draft-preview-updated` listener.
- Bubble copy priority: `src/App.svelte` `genieBubble`.
- Active MCP status summary: `src/lib/agents/activity.ts` `resolveActiveMcpBubble`.
- Bubble render/size: `src/lib/VertexGenie.svelte` `.genie-bubble`.
- Terminal stream path: `src-tauri/src/mcp/runtime.rs` -> `agent-terminal-updated` -> `src/lib/stores/agentTerminalStore.ts` -> `src/lib/AgentTerminalSurface.svelte`.
- Structural verification already exists after preview render and is included in compact MCP tool responses.

### Phase 1: Make Draft Feedback Eventful

- Add optional validation/feedback payload to draft preview event.
- Payload should be compact and camelCase at TS boundary:
  - `status`: `checking | passed | failed | warning`
  - `summary`: one short line for bubble
  - `items`: bounded list of issue/evidence rows
  - `source`: `structuralVerification | renderError | toolError | visualRepair`
  - `previewId`, `threadId`, `sessionId`
- Backend fields stay snake_case; boundary structs use `#[serde(rename_all = "camelCase")]`.
- Store latest feedback with durable `agent_drafts`, so restart/session-memory loss does not lose visible draft context.
- Do not create history messages for validation chatter.

Acceptance:

- Given MCP preview render passes structural verification, when event reaches frontend, bubble can show concise pass summary.
- Given MCP preview render has structural issues, bubble can show first issue plus count.
- Given app reloads after draft preview, latest draft feedback can be recovered with draft.

### Phase 2: Bubble Becomes Compact Status Surface

- Shrink default bubble footprint.
- Keep bubble near pet, but prevent overlap with top-right buttons.
- Prefer one-line status by default; expand/copy action only when detail exists.
- Keep full validation detail in a panel/terminal-like surface, not always-open bubble text.
- Lower risk first change: CSS only in `VertexGenie.svelte`.
- Button overlap fix options:
  - reduce `.genie-bubble` width/offset/max-height
  - reserve right-side overlay space
  - ensure `.genie-layer` cannot cover `.app-overlay-actions`

Acceptance:

- Desktop: bubble does not cover audio/terminal/draw/settings buttons.
- Narrow viewport: bubble fits or collapses instead of clipping offscreen.
- Copy/close controls remain reachable.
- Existing onboarding/confirm/screenshot prompts still win priority.

### Phase 3: Feedback Priority Model

Bubble priority should become:

1. Blocking human choice: screenshot prompt, confirm prompt, pending terminal input.
2. Active agent validation feedback for current draft.
3. Active MCP activity label/status.
4. Thread error summary.
5. Repair/cooking/advisor text.

Validation feedback should expire or demote when:

- newer preview arrives
- preview is committed
- user switches thread/model
- user dismisses exact current feedback

Acceptance:

- Fresh validation feedback replaces generic "Preview rendered."
- Dismiss hides current feedback but next preview can show new feedback.
- Committed preview clears draft-only feedback.

### Phase 4: Errors Split Into Bubble Summary + Terminal Detail

- Bubble shows short error: tool name + first raw error sentence.
- Agent Terminal holds raw stdout/stderr tail.
- App logs get bounded/redacted event detail only.
- Fix current risk where agent exit error can include `Last agent output` in status/log/bubble.
- Do not surface agent iteration errors as normal assistant timeline messages unless they create a useful artifact.

Acceptance:

- MCP tool validation error appears in bubble as concise model-visible feedback.
- Raw terminal tail does not enter app logs.
- Agent Terminal still has raw stream for debugging.

### Phase 5: Sprite/Pet-Friendly Renderer

- Keep `VertexGenie.svelte` public behavior.
- Add optional visual renderer mode later:
  - current procedural canvas remains fallback
  - sprite atlas renderer can use same `mode`, `intensity`, `agentConnected`
  - bubble/status logic stays outside sprite renderer
- Persist sprite identity only after UI proof; do not start backend config churn first.

Acceptance:

- Existing procedural pet still works.
- Sprite mode can animate idle/thinking/rendering/error without changing bubble event contracts.

### BDD Slices

1. Outer integration: MCP preview render emits draft feedback, viewport updates, bubble shows feedback.
2. Unit: compact validation summary maps pass/warn/fail into bounded bubble text.
3. Unit: bubble priority picks validation feedback above generic active MCP status.
4. UI integration: small bubble does not overlap top-right controls on desktop and narrow viewport.
5. Backend unit: terminal exit errors store raw tail only in terminal snapshot, not app log/status text.

### Non-Goals

- No separate agent status bar.
- No terminal stream in app logs.
- No history message per validation step.
- No full raw validation dump in always-visible bubble.
- No sprite persistence until renderer proves useful.
