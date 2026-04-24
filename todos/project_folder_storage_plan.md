# Project Folder Storage Plan

## Status
- [ ] Not complete.
- [ ] Planning doc only. No storage implementation yet.

## Decision
Store threads inside user-owned project folders.

Use one sqlite database per project:

```text
<project>/
  .ecky/
    project.json
    project.sqlite
    model-runtime/
      <thread-id>/
        <version-id>/
          manifest.json
          model.FCStd
          preview.stl
          viewer-assets/
  exports/
```

Keep only a global project index in app config:

```text
<app-config>/projects.json
```

Global index stores recent projects, active project, and missing-folder state. It must not store thread/message/model data.

## Current Shape
- [ ] Backend opens one global `history.sqlite` from app config in `src-tauri/src/lib.rs`.
- [ ] `AppState` owns one shared sqlite connection in `src-tauri/src/models.rs`.
- [ ] Thread/message/history commands query that global connection.
- [ ] Frontend `ProjectSwitcher.svelte` is a thread switcher, not a real folder project switcher.
- [ ] Runtime artifacts are app-global under `model-runtime`.
- [ ] Artifact bundles currently carry absolute paths. That hurts project portability.

## Target Invariants
- [ ] User folder is project root.
- [ ] `.ecky/project.sqlite` owns all threads for that project.
- [ ] Thread ids remain UUIDs. No new id scheme needed.
- [ ] Project id is stored once in `.ecky/project.json` and mirrored in sqlite `project_meta`.
- [ ] One active project is loaded in v1.
- [ ] App config stores recent project pointers only.
- [ ] Project DB schema reuses existing thread/message/version tables where possible.
- [ ] New artifact paths are stored project-relative when durable.
- [ ] Old absolute artifact paths still load during migration.
- [ ] Export can package a whole project folder without scraping global app config.

## Non-Goals
- [ ] No rewrite of render stack.
- [ ] No new storage engine.
- [ ] No multi-project open state in v1.
- [ ] No cloud sync semantics.
- [ ] No renaming backend `thread` domain yet.
- [ ] No forced migration that risks deleting current `history.sqlite`.

## Phase 0 - Baseline Tests And Seams
- [ ] Add e2e spec: Given one project with one thread, When app opens project, Then thread appears.
- [ ] Add e2e spec: Given two projects with different threads, When switching projects, Then thread list changes scope.
- [ ] Add e2e spec: Given missing recent project folder, When app boots, Then project index shows missing state without crash.
- [ ] Add Rust unit tests around project path validation.
- [ ] Add Rust unit tests around project index read/write.
- [ ] Add Rust unit tests around project DB init using existing migrations.
- [ ] Confirm current global history tests still pass before refactor.

Verification:
- [ ] `npm run test:e2e -- e2e/project-storage.spec.ts`
- [ ] `cd src-tauri && cargo test project_index --lib`
- [ ] `cd src-tauri && cargo check`

## Phase 1 - Project Index
Add global recent-project index without moving thread storage yet.

Backend:
- [ ] Add `src-tauri/src/projects.rs`.
- [ ] Add `ProjectRecord` with `#[serde(rename_all = "camelCase")]`.
- [ ] Add `ProjectIndex` with `schema_version`, `active_project_id`, `projects`.
- [ ] Add commands:
  - [ ] `list_projects()`
  - [ ] `create_project(path, name)`
  - [ ] `open_project(path)`
  - [ ] `set_active_project(project_id)`
  - [ ] `forget_project(project_id)`
- [ ] Validate paths are directories.
- [ ] Create `.ecky/` on project creation.
- [ ] Create `.ecky/project.json` with stable UUID.
- [ ] Mark missing folders instead of deleting records.

Frontend:
- [ ] Add `src/lib/stores/projects.ts`.
- [ ] Add typed Tauri wrappers using camelCase payloads.
- [ ] Show recent projects in existing project switcher window.
- [ ] Keep existing global thread behavior until Phase 3.

Acceptance:
- [ ] User can create/select a project folder.
- [ ] Active project persists across restart.
- [ ] Missing folder is visible as missing, not fatal.

## Phase 2 - Active Project Context
Introduce active project routing while still allowing legacy global DB.

Backend:
- [ ] Add `ProjectContext`:
  - [ ] `project_id`
  - [ ] `root_dir`
  - [ ] `meta_dir`
  - [ ] `db_path`
  - [ ] `artifact_root`
- [ ] Add active project state to `AppState`.
- [ ] Add helper for DB access:
  - [ ] `with_active_project_db(...)`
  - [ ] `with_legacy_db(...)` fallback for compatibility
- [ ] Keep one active sqlite connection open.
- [ ] Prevent project switching while a render/generation commit is mutating persistence.
- [ ] Add raw error bodies for project open failures.

Frontend:
- [ ] Store active project id separately from active thread id.
- [ ] Clear active thread when active project changes.
- [ ] Reload thread inventory after project switch.

Acceptance:
- [ ] Switching project changes active project state.
- [ ] Existing thread commands still work in legacy mode.
- [ ] Project switch cannot corrupt in-flight render state.

## Phase 3 - Per-Project SQLite
Move thread data into `.ecky/project.sqlite`.

Backend:
- [ ] Reuse existing `db::init_db` migrations for project sqlite.
- [ ] Add `project_meta` table:

```sql
CREATE TABLE IF NOT EXISTS project_meta (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);
```

- [ ] Store `project_id`, `project_name`, and `schema_version` in `project_meta`.
- [ ] Route thread commands through active project DB.
- [ ] Route message commands through active project DB.
- [ ] Route window layout commands through active project DB.
- [ ] Route agent session/target lease tables through active project DB if they are thread-bound.
- [ ] Keep truly app-global config outside project DB.
- [ ] Add explicit no-active-project error for thread commands.

Frontend:
- [ ] Disable create/generate UI when no active project exists.
- [ ] Display no-project empty state with create/open actions.
- [ ] Thread switcher lists only active project threads.

Acceptance:
- [ ] Project A thread never appears in Project B.
- [ ] Creating a thread writes only to active project sqlite.
- [ ] Restart restores active project and scoped thread list.

## Phase 4 - Project-Local Artifacts
Move canonical generated artifacts into the active project folder.

Backend:
- [ ] Add project-aware path resolver or resolver overlay.
- [ ] Route generated runtime bundles to `.ecky/model-runtime/<thread-id>/<version-id>/`.
- [ ] Store durable artifact paths relative to project root.
- [ ] Keep compatibility loader for existing absolute artifact paths.
- [ ] Ensure `FCStd + manifest.json` stay canonical per `todos/model_artifact_strategy.md`.
- [ ] Keep app-global runtime dependencies/cache separate from project artifacts.
- [ ] Make export paths deterministic under `<project>/exports/`.

Acceptance:
- [ ] New render creates artifacts under active project folder.
- [ ] Moving project folder preserves new artifact references.
- [ ] Legacy absolute-path versions still preview if files still exist.

## Phase 5 - UI Project Folder UX
Make project semantics visible.

Frontend:
- [ ] Rename current thread switcher copy where needed:
  - [ ] `Projects` means folders.
  - [ ] `Threads` or `Models` means designs inside active project.
- [ ] Add active project header with folder name.
- [ ] Add actions:
  - [ ] New Project
  - [ ] Open Project
  - [ ] Reveal In Finder
  - [ ] Forget Recent
  - [ ] Rename Project
- [ ] Keep Tactical Midnight styling.
- [ ] Keep major containers `overflow: hidden`.
- [ ] No separate agent status bar.

BDD:
- [ ] Given no active project, user can create project and sees empty thread list.
- [ ] Given active project, user can create design and it appears in that project.
- [ ] Given second project, user can switch and original design disappears from list.
- [ ] Given missing project path, user sees actionable missing state.

Verification:
- [ ] `npm run test:unit`
- [ ] `npm run test:e2e`
- [ ] Browser proof on real routes/app flow.

## Phase 6 - Migration
Move old global history into a project safely.

Backend:
- [ ] Detect legacy `<app-config>/history.sqlite`.
- [ ] Add command `inspect_legacy_history()`.
- [ ] Add command `migrate_legacy_history_to_project(project_path)`.
- [ ] Copy, do not move, legacy data first.
- [ ] Verify migrated thread/message counts.
- [ ] Rewrite artifact paths only when copied successfully.
- [ ] Leave old `history.sqlite` as backup.
- [ ] Mark migration completed in app config after verification.

Frontend:
- [ ] On boot, if legacy history exists and no project exists, offer:
  - [ ] Create project from existing history.
  - [ ] Keep using legacy temporarily.
  - [ ] Start empty project.
- [ ] Show migration count summary before action.
- [ ] Show raw backend migration error if failed.

Acceptance:
- [ ] Legacy users do not lose history.
- [ ] Migrated project opens with same visible threads.
- [ ] Migration can be retried.
- [ ] User can still manually export old data if migration fails.

## Phase 7 - Export And Packaging
Make grouping/export sane.

Project export:
- [ ] Zip project root or `.ecky/` plus selected visible exports.
- [ ] Include `project.json`, `project.sqlite`, manifests, FCStd files, previews, viewer assets.
- [ ] Exclude dependency caches and temp files.

Thread export:
- [ ] Export one thread with source, manifest, FCStd, preview STL, and metadata.
- [ ] Preserve relative paths inside package.

Acceptance:
- [ ] Exported project imports on clean machine.
- [ ] Exported thread imports into another project.
- [ ] No dependency cache required for viewing existing preview artifacts.

## Phase 8 - Cleanup
- [ ] Remove direct uses of global `state.db` from thread-scoped code.
- [ ] Keep global config DB/file only for app settings and project index.
- [ ] Remove legacy fallback after at least one safe migration release.
- [ ] Update architecture docs to state project folder ownership.
- [ ] Update support/debug UI to show active project path and sqlite path.

## Risk Register
- [ ] Absolute artifact paths break portability. Fix with project-relative writes plus legacy absolute reads.
- [ ] Long-lived DB lock during render can freeze project switch. Fix with short DB transactions and render outside DB lock.
- [ ] Project folder moved outside app. Fix with missing state and reopen-location action.
- [ ] Thread id collisions unlikely but possible in imports. Fix by remapping imported thread ids on conflict.
- [ ] Agent/MCP runtime may assume global history. Fix by passing active project path and DB context through target metadata.
- [ ] App-global runtime dependencies must not be copied into project. Keep caches separate from canonical artifacts.

## First Implementation Slice
- [ ] Write Phase 1 project index tests.
- [ ] Implement project index read/write.
- [ ] Add create/open/list project commands.
- [ ] Add frontend store and minimal project selector UI.
- [ ] Leave thread storage global until Phase 3.
- [ ] Run:
  - [ ] `npm run test:unit`
  - [ ] targeted project e2e
  - [ ] `cd src-tauri && cargo check`

