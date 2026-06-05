# Tasks: Filesystem Project Mirror

## Worker Rules

- `.ecky` source stays canonical; folder is a mirror.
- No direct SQLite writes; version writes go through existing preview/commit
  handlers only.
- Tauri boundary structs use `#[serde(rename_all = "camelCase")]`.
- TDD: failing test first per slice; `cd src-tauri && cargo check` before any
  success claim.
- No commits/staging.

## 1. T1 - Mirror Core

Write scope: new `src-tauri/src/project_mirror.rs`, `src-tauri/src/lib.rs`
(module registration), `src-tauri/src/contracts.rs` (Config field), tests.

- [x] 1.1 Manifest read/write round-trip (`ecky-project.json`, camelCase,
  schemaVersion 1).
- [x] 1.2 Export: write `model.ecky` + manifest into
  `<projectsRoot>/<slug>/`; deterministic slug; re-export refreshes.
- [x] 1.3 Status classification: missing / clean / fileChanged /
  threadAdvanced / conflict, pure function over (file digest, manifest,
  thread head id).
- [x] 1.4 `projectsRoot` config field with `<app_data>/projects` default.
  (Added `Config.projects_root: Option<String>` (camelCase `projectsRoot`,
  serde default None); `projects_root()` honors a non-blank override and
  otherwise falls back to `<app_data>/projects`; threaded through
  `project_dir`/`folder_status`/`list_project_slugs`/`export_project` and all
  handlers, the watcher, and `open_project_in_editor`. BDD:
  `projects_root_honors_config_override_else_defaults`.)

## 2. T2 - Apply Flow (MCP handlers)

Write scope: `src-tauri/src/mcp/handlers.rs`, tests.

- [x] 2.1 `handle_project_folder_export`: resolve bound target (thread or
  explicit message), write mirror, return folder path + manifest.
- [x] 2.2 `handle_project_folder_status`: read-only classification incl.
  thread head lookup.
- [x] 2.3 `handle_project_folder_apply`: digest gate -> compile/preview via
  `handle_macro_preview_render` -> `handle_commit_preview_version` -> manifest
  rebase; conflict/threadAdvanced refusal; `force` override.
- [x] 2.4 Failure paths: invalid source surfaces raw compiler error, folder
  and thread untouched.

## 3. T3 - MCP Tool Surface

Write scope: `src-tauri/src/mcp/server.rs`, tests.

- [x] 3.1 Register `project_folder_export` / `project_folder_status` /
  `project_folder_apply` tool definitions with camelCase schemas.
- [x] 3.2 Dispatch arms wired to T2 handlers.
- [x] 3.3 Tool-definition test coverage.

## 4. T4 - Docs

- [x] 4.1 Authoring card: external-editor/LLM flow (export -> edit file ->
  apply) and the no-direct-DB rule.
- [x] 4.2 ecky-ir book: "Projects as folders" section documenting the folder
  contract and conflict semantics.

## 5. T5 - Watcher and UI Surfacing (later)

- [x] 5.1 File watcher (debounced) emitting status changes to the UI.
  (1s polling loop, two-tick digest settle, per-digest failure memo;
  emits `history-updated` + `project-folder-sync` events.)
- [ ] 5.2 Status chip + export/apply affordances in the app shell.
- [ ] 5.3 Playwright BDD: happy path + conflict path on a real route.

## 6. T6 - Literate Document Renderer (rides macro-ast-map-editor)

- [ ] 6.1 Document-skin renderer over the AstMap projection (same node ids,
  same patch intents, document layout); tracked in macro-ast-map-editor
  phases, listed here for traceability.

## Proof Gates

- [x] G-MIRROR Export -> external edit -> apply yields a new committed version
  with rendered preview; manifest rebased.
- [x] G-STALE threadAdvanced and conflict refuse without force, with exact
  reasons.
- [x] G-GREEN Full `cargo test` stays green.
