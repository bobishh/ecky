# Design: Filesystem Project Mirror

## Architecture

```text
thread/version (sqlite history, canonical record)
        |  project_folder_export
        v
<projectsRoot>/<slug>/
  model.ecky          <- editable by anything: editors, LLM file skills, sed
  ecky-project.json   <- binding manifest, written only by Ecky
        |  external edit
        v
project_folder_status  (digest classification, read-only)
        |  project_folder_apply
        v
compile check -> macro_preview_render -> commit_preview_version
        |
        v
new version message on the bound thread; manifest rebased to it
```

The folder is a mirror, never an alternate database. Every write into history
flows through the same preview/commit handlers agents already use, so leases,
artifact truth, and version provenance behave identically regardless of who
edited the file.

## Manifest contract (`ecky-project.json`)

```json
{
  "schemaVersion": 1,
  "projectId": "proj-<uuid>",
  "threadId": "thread-...",
  "messageId": "msg-...",
  "modelId": "generated-...",
  "sourceDigest": "sha256:<hex of exported model.ecky bytes>",
  "exportedAt": 1781200000
}
```

- camelCase (Tauri boundary convention for JSON contracts).
- `sourceDigest` is the digest of the bytes Ecky last wrote or applied; it is
  the only thing distinguishing "user edited the file" from "clean".
- Ecky owns the manifest; external editors must not need to touch it.

## Status classification

```text
missing        no model.ecky or no manifest
clean          file digest == manifest.sourceDigest, thread head == manifest.messageId
file_changed   file digest != manifest.sourceDigest, thread head == manifest.messageId
thread_advanced file digest == manifest.sourceDigest, thread head != manifest.messageId
conflict       both differ
```

Thread head = latest committed assistant version message on the bound thread.
Status is read-only and cheap (one digest + one history lookup).

## Apply semantics

- `file_changed` -> compile check, preview render, commit, manifest rebased.
- `clean` -> no-op success (idempotent).
- `thread_advanced` -> error telling the caller to re-export (folder is stale).
- `conflict` -> error unless `force: true`; force applies the file as a new
  version on top of the current head (file wins, history preserved as
  versions; nothing is lost because the previous head remains a version).
- Compile or render failures surface the raw error and leave both the folder
  and the thread untouched.

## Ownership

- Backend: `src-tauri/src/project_mirror.rs` (manifest io, digesting, status
  math, slug rules) + `mcp/handlers.rs` glue (thread head lookup, preview and
  commit composition) + `mcp/server.rs` tool registration.
- Config: `projectsRoot` (empty -> `<app_data>/projects`), persisted via the
  normal `save_config` flow.
- Frontend: none in this change. Later phases surface status in the UI and add
  an export action; the literate renderer is a macro-ast-map-editor phase.

## Literate projection note

The document/"literate programming" editing mode is the AstMap projection
rendered as a nested document instead of a spatial scene: same stable node
ids, same patch intents (`PatchParamValue`, `InsertNode`, ...), different
layout. It must not become a separate editor with its own identity model.
Recorded here so the folder mirror, the spatial map, and the document skin
stay three views over one source-backed AST.

## Risks

- Slug collisions across threads -> deterministic slug from thread id suffix.
- Folder edited while apply runs -> apply re-digests the bytes it actually
  read and stores that digest; a concurrent edit becomes the next
  `file_changed`.
- Users deleting the manifest -> status reports `missing`; re-export repairs.
