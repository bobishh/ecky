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

- [x] 1.1 Manifest read/write round-trip (`ecky-project.edn`, kebab-case EDN,
  schema-version 1).
- [x] 1.2 Export: write `model.ecky` + manifest into
  `<projectsRoot>/<slug>/`; deterministic slug; re-export refreshes.
- [ ] 1.3 Lossless status/appending semantics: clean / fileChanged / missing
  plus informational head movement; no conflict classification gate.
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
- [ ] 2.3 `handle_project_folder_apply`: changed bytes append a version/head
  before compile/preview; preserve success/failure outcome and manifest.
- [ ] 2.4 Failure paths: invalid source surfaces raw error while failed version
  and exact source remain queryable; unchanged source is idempotent.

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
- [x] 5.1a Derive watcher eligibility from active UI and live MCP authoring
  targets, independent of render/version state. Prove first-version success,
  failed-source preservation, and inactive-folder isolation.
- [ ] 5.2 Status chip + export/apply affordances in the app shell: remove
  conflict/force refusal UI; show failed head and success-only filter.
  (Tauri commands `project_folder_export` / `project_folder_status` /
  `project_folder_apply` in `src-tauri/src/commands/project_mirror.rs`, thin
  wrappers over the existing mirror core + MCP `project_folder_*` handlers
  so all version writes flow through preview -> verify -> commit (no direct
  DB writes); registered in `bindings.rs`. Frontend wrappers in
  `src/lib/tauri/client.ts` + regenerated `contracts.ts`. New
  `src/lib/ProjectFolderStatusChip.svelte` rendered above the ParamPanel
  mode tabs, refreshed on thread/version switch and on `project-folder-sync`
  / `history-updated` events; surfaces state
  (clean/fileChanged/threadAdvanced/conflict/missing) plus
  EXPORT / APPLY / RE-EXPORT / FORCE APPLY affordances and the raw backend
  error verbatim. BDD: `e2e/project-folder-sync.spec.ts`.)
- [ ] 5.3 Playwright BDD: export -> edit -> apply appends head; invalid edit
  still appends failed head; both-sides changed serializes file as newest;
  unchanged apply creates no duplicate; success-only filter excludes failures.

## 6. T6 - Literate Document Renderer (rides macro-ast-map-editor)

- [x] 6.1 Document-skin renderer over the AstMap projection (same node ids,
  same patch intents, document layout); tracked in macro-ast-map-editor
  phases, listed here for traceability.
  (New `src/lib/MacroAstDocument.svelte` is an alternate LAYOUT over the
  existing `buildMacroAstMapProjection` — identical stable node ids, identical
  patch intents (`onUpdate` / `onDraftValue` / `onApplyMacroCode` /
  `onControlFocusChange`) reused from `MacroAstMap.svelte`; only the layout
  differs (nested document vs spatial scene). No second identity model, no
  embedded fake data, no direct DB writes. A MAP/DOC sub-toggle in
  `ParamPanel`'s `newParams` branch switches layouts; `astViewMode` is
  session-only view state. Theme: Tactical Midnight, square borders, bronze
  accents, `overflow: hidden`. BDD: `e2e/ast-document-skin.spec.ts` —
  happy path proves DOC node ids == MAP node ids for the same macro and an
  inline param edit in DOC view emits the shared `render_model` patch via
  Apply; failure path proves a render failure after a DOC-view inline edit
  surfaces the raw backend error verbatim in the session bubble.)

## Proof Gates

- [x] G-MIRROR Export -> external edit -> apply yields a new committed version
  with rendered preview; manifest rebased.
- [ ] G-LOSSLESS changed or invalid source is never lost; newest appended
  version is always head and success filtering is separate.
- [x] G-GREEN Full `cargo test` stays green.
