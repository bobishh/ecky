# Proposal: Filesystem Project Mirror

## Intent

Expose Ecky projects as plain folders on the user's filesystem so any editor,
tool, or LLM with a file skill can author `.ecky` source directly, while Ecky
remains the renderer, validator, and history-keeping workbench rather than the
primary code editor.

Threads and versions stay the durable record. A project folder is a mirror of
one thread's active macro: editable outside the app, re-entering the app only
through the existing compile -> preview -> commit pipeline. The `New Params`
direction (see `macro-ast-map-editor`) then reads the same source projection;
this change adds the document/literate skin to that view's roadmap without
duplicating its contracts.

## Scope

- One directory per project under a configurable projects root
  (`config.projectsRoot`, default `<app_data>/projects`).
- Folder contents: `model.ecky` (active macro source) plus `ecky-project.json`
  (camelCase manifest binding the folder to threadId/messageId/modelId with a
  sourceDigest of the exported text).
- Explicit, digest-based sync semantics:
  - export: write/refresh the folder from a thread's active version.
  - status: classify the folder as clean / fileChanged / threadAdvanced /
    conflict / missing without mutating anything.
  - apply: compile-check the edited file, render a preview, commit it as a new
    version on the bound thread, and refresh the manifest.
- Conflict policy: `apply` refuses when the thread advanced past the manifest
  binding unless the caller passes an explicit force flag; conflicts never
  silently clobber either side.
- MCP tools `project_folder_export`, `project_folder_status`,
  `project_folder_apply` so agents and external editors share one flow; all
  version writes go through existing preview/commit handlers (no direct DB
  writes).
- Literate-document rendering of the macro (prose-like nested document where
  each AST node is an editable block) is recorded here as a second renderer
  over the `macro-ast-map-editor` AstMap projection, gated behind that change's
  Phase 1 identity contract.

## Out of Scope

- Replacing threads/history with the filesystem as source of truth.
- Live bidirectional file watching (follow-up task once status semantics are
  proven; first slice is explicit sync).
- Multi-file projects / `include` mechanics (single `model.ecky` plus the
  existing component-library flow for reuse).
- Git integration, sync over network shares, collaborative merge.
- Building the literate renderer UI (specced direction only; implementation
  rides macro-ast-map-editor phases).
- Removing or changing the existing params panel or editor.

## Approach

- T1: `project_mirror` core in Rust: manifest contract, export, digest status,
  deterministic conflict classification. Pure fs + digest logic, no DB.
- T2: MCP handlers composing existing `macro_preview_render` +
  `commit_preview_version` flows for `apply`; thread binding checks against
  history.
- T3: MCP tool registration + dispatch.
- T4: docs: authoring-card note + ecky-ir book section describing the folder
  contract for external editors and LLM skills.
- T5 (later): file watcher + in-app surfacing (status chip, re-export prompt).
- T6 (later, in macro-ast-map-editor): literate document renderer as an
  alternate projection skin.

## Success Criteria

- A user can export a thread to a folder, edit `model.ecky` in any editor,
  run `project_folder_apply` (directly or via an agent), and see a new
  committed version with a rendered preview in the app.
- A stale folder (thread advanced) refuses to apply without force and reports
  exactly why.
- All sync states are observable through `project_folder_status` without side
  effects.
- Existing storage, params panel, and editor behavior unchanged.
