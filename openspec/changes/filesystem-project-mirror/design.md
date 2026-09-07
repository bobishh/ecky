# Design: Filesystem Project Mirror

## Architecture

```text
thread/version (sqlite history, canonical record)
        |  project_folder_export
        v
<projectsRoot>/<slug>/
  model.ecky          <- editable by anything: editors, LLM file skills, sed
  ecky-project.edn   <- binding manifest, written only by Ecky
        |  external edit
        v
project_folder_status  (digest classification, read-only)
        |  project_folder_apply
        v
append exact file bytes as version/head -> compile check -> macro_preview_render
        |
        v
new version message on the bound thread (success or failure); manifest rebased
to it when apply completes
```

The folder is a mirror, never an alternate database. Every write into history
flows through the same preview/commit handlers agents already use, so leases,
artifact truth, and version provenance behave identically regardless of who
edited the file.

## Manifest contract (`ecky-project.edn`)

```clojure
{:exported-at 1781200000
 :message-id "msg-..."
 :model-id "generated-..."
 :project-id "proj-<uuid>"
 :schema-version 1
 :source-digest "sha256:<hex of exported model.ecky bytes>"
 :thread-id "thread-..."}
```

- Kebab-case EDN on disk; camelCase remains only at the Tauri boundary.
- `source-digest` is the digest of the bytes Ecky last wrote or applied; it is
  the only thing distinguishing "user edited the file" from "clean".
- Ecky owns the manifest; external editors must not need to touch it.

## Status classification

```text
missing        no model.ecky or no manifest
clean           file digest == manifest.source-digest
file_changed   file digest != manifest.source-digest
missing         no model.ecky or no manifest
thread_moved    informational flag: head != manifest.message-id
```

Thread head = latest appended version record on the bound thread, independent
of validation/render status.
Status is read-only and cheap (one digest + one history lookup).

## Apply semantics

- `file_changed` -> append exact source as a new version/head, then compile and
  render it; persist success or failure on that version.
- `clean` -> no-op success (idempotent).
- `thread_moved` plus changed file -> append file as newest head; no conflict.
- Compile/render failures surface raw error but retain failed version/source.
  Exact unchanged content is deduplicated.

## Ownership

- Backend: `src-tauri/src/project_mirror.rs` (manifest io, digesting, status
  math, slug rules) + `mcp/handlers.rs` glue (thread head lookup, preview and
  commit composition) + `mcp/server.rs` tool registration.
- Config: `projectsRoot` (empty -> `<app_data>/projects`), persisted via the
  normal `save_config` flow.
- Frontend: none in this change. Later phases surface status in the UI and add
  an export action; the literate renderer is a macro-ast-map-editor phase.

## Watcher eligibility

The watcher consumes a set of active authoring thread identities. The set is
the union of the selected UI thread and live MCP session targets. Render
snapshots remain viewport evidence only; they do not decide whether source may
enter history. This keeps the first append for a blank bound thread on the same
path as every later append, without a no-version bootstrap branch.

Inactive folders remain ignored. Multiple active authoring surfaces may own
different threads; the existing apply lock serializes their builds.

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
