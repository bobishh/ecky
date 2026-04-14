# Floating Project Workspace Plan

## Status
- [ ] Not complete.
- [x] Before resuming implementation, inspect current diff. Backend layout structs/table helpers may already be partially patched from an interrupted implementation pass.

## Summary
Refactor Ecky UI without a domain rename. Treat `thread = project` in user-facing UI, keep Rust/DB names as `thread`. Remove the left tiled sidebar. Move navigation, parameters, dialogue, settings, and terminal into floating native Svelte windows with Cultiwars-style drag/resize/z-order, but Ecky bubble styling. Do not use iframes for native panels.

## Window System
- [x] Replace current simple `Window.svelte` with a Cultiwars-derived shell:
  - internal drag and resize state
  - viewport clamp
  - active z-order
  - close button
  - resize handle
  - glass pane during drag/resize
- [x] Style windows like `genie-bubble`:
  - square geometry
  - translucent `var(--bg-100)`
  - primary-mixed border
  - mono header
  - shadow and backdrop blur
- [x] Add `windowRegistry` static TS definitions for:
  - `projects`
  - `params`
  - `dialogue`
  - `settings`
  - `terminal`
- [x] Each registry item defines:
  - title
  - default rect
  - min size
  - mount policy
- [x] Add window store:
  - visible
  - minimized
  - rect
  - z
  - active state
  - `bringToFront`
  - `toggleWindow`
  - `closeWindow`
  - `updateRect`
  - viewport clamp
- [x] `window ring` means z-order stack only. No visible ring UI in v1.

## DB Layout Persistence
- [x] Add table:

```sql
CREATE TABLE IF NOT EXISTS thread_window_layouts (
  thread_id TEXT PRIMARY KEY,
  layout_json TEXT NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY(thread_id) REFERENCES threads(id) ON DELETE CASCADE
);
```

- [x] Add Rust contract structs with `#[serde(rename_all = "camelCase")]`:
  - `ThreadWindowLayout`
  - `ThreadWindowState`
- [x] Add Tauri commands:
  - `get_thread_window_layout(thread_id) -> Option<ThreadWindowLayout>`
  - `save_thread_window_layout(thread_id, layout) -> ()`
- [x] Frontend loads layout on project/thread switch.
- [x] Merge DB layout with `windowRegistry` defaults.
- [x] Ignore stale layout responses via thread switch token.
- [x] Persist dirty layouts every 2 seconds.
- [x] Hard flush on:
  - thread switch
  - window close
  - app unload
- [x] Never write layout to DB on mousemove.

## Dock And Panels
- [x] Add top-right dock primary group:
  - `Projects`
  - `Parameters`
  - `+`
- [x] Keep utility group for existing behavior:
  - draw
  - audio/microwave
  - terminal
  - settings
- [x] Remove standalone Inventory and Trash buttons. They move into Projects overlay tabs.
- [x] `+` opens the current create flow. No new entity split:
  - blank project/thread
  - import FCStd
  - import macro/Python
- [x] `Parameters` toggles params window for current model/version.
- [x] `Projects` toggles project switcher window.
- [ ] `Settings` toggles settings window, not full-page view.
- [x] Transient modals stay modal:
  - import macro
  - delete confirm
  - code modal
  - export chooser
  - onboarding

## Project Switcher
- [ ] Add unified project switcher window replacing:
  - `Thread History`
  - `InventoryPanel`
  - `DeletedModels`
- [ ] Tabs:
  - `In Work`, backed by `getHistory`
  - `Archived`, backed by `getInventory`
  - `Trash`, backed by `getDeletedMessages`
- [ ] Search filters current tab by:
  - title
  - summary
  - model title
  - version
- [ ] Use one card/grid style based on current Trash UI for active, archived, and trash.
- [ ] Active cards support:
  - open
  - rename
  - archive
  - delete
- [ ] Archived cards support:
  - open final model
  - reopen
- [ ] Trash cards support:
  - recover
  - hide
- [ ] Fix archived previews:
  - lazy fetch `getThreadLatestVersion(threadId)` for visible active/archived cards
  - use latest version `imageData`
  - cache by thread/update key
  - Trash continues using `DeletedMessage.imageData`
  - show stable placeholder with title/version/backend when no image exists

## Lazy Behavior
- [ ] Viewer remains base layer and stays visible during project/thread switch.
- [ ] `params`, `projects`, and `settings` mount only when visible.
- [ ] `dialogue` and `terminal` use keep-alive after first open.
- [ ] Existing lazy message/version loading remains:
  - token guard
  - dialogue preloader
  - stale results ignored

## Test Plan
- [x] Frontend unit: window store merges DB layout with registry defaults and clamps bad rects.
- [ ] Frontend unit: z-order stack activates clicked/toggled window without duplicates. (store functions exist, test coverage pending)
- [ ] Frontend unit: dirty tick saves once per dirty layout at 2s, hard flush runs on thread switch.
- [ ] Frontend unit: dock toggles Projects/Parameters and `+` opens old create chooser.
- [ ] Frontend unit: hidden `params` does not mount ParamPanel, viewer stays mounted during switch.
- [ ] Frontend unit: project switcher filters tabs and ignores stale preview loads.
- [ ] Frontend unit: archived cards render preview from latest version `imageData` or placeholder fallback.
- [x] Rust unit: DB migration creates `thread_window_layouts`.
- [x] Rust unit: save/get layout roundtrip works for one thread.
- [x] Rust unit: missing thread returns not-found on save.
- [x] Rust unit: get layout returns `None` when thread has no layout.
- [x] Rust unit: deleting/soft-deleting thread does not break layout reads for other threads.

## Verification Commands
- [ ] `npm run typecheck`
- [ ] `npm run test:unit`
- [ ] `cd src-tauri && cargo check`
- [ ] `cd src-tauri && cargo test thread_window_layout --lib`

## Assumptions
- [ ] No new backend `Project`, `Model`, `Plate`, or `Assembly` entity now.
- [ ] User-facing nav copy says `Project`.
- [ ] Backend and internal IDs can stay `threadId`.
- [ ] No iframe for Params/Dialogue/Settings v1.
- [ ] Sidebar removed now, not kept behind fallback flag.
