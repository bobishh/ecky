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

3. **Enhance `mcp_ecky_mcp_params_patch_and_render`:**
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
3. `params_patch_and_render({ "wall_thickness": 3.0 })`

## Next Steps
- Implement the Tauri event emitter in the MCP server.
- Wire up the event listener in Svelte `App.svelte` or a global layout.
- Test the CSS animation on sliders.
