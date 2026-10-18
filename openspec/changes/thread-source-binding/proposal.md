# Proposal: Thread Source Binding

## Intent

Keep existing global Ecky history. Give every thread a real, user-visible
source file from first creation, under a user-selected source root. The file is
usable by an editor and by an agent with filesystem access. Ecky keeps SQLite
thread/version history and generated artifacts exactly as today.

This is not a workspace-storage rewrite and not a Git requirement.

## Current State

```text
<app-data>/history.sqlite
  threads + messages + versions + macro_code

<app-data>/model-runtime/<kind>/<model-id>/
  model.stl, model.FCStd, manifest.json, bundle.json
```

Existing folder support is an on-demand export of one thread into
`<projectsRoot>/<slug>/model.ecky`. It derives the folder every time and hides
the only editor action as `FILE ↗` in Params. It must become a persistent,
visible source binding, not a special export flow.

## Decision

```text
global SQLite                    canonical Ecky history
app-data/model-runtime           STL / FCStd / previews
configured source root           normal user-visible folders
<source-root>/<thread-slug>/
  model.ecky                     editable working copy
  ecky-project.edn              existing mirror manifest + digest
```

- `thread_source_bindings` persists thread ID, absolute folder, source path,
  digest, and timestamps. A thread keeps its folder after rename/root change.
  `stable-project-folder-names` adds a startup-only migration of exact legacy
  `untitled-design-<id>` placeholders; subsequent title changes still retain paths.
- Existing `ecky-project.edn` schema v1 remains the only folder sidecar.
  No schema bump, second manifest, or per-version parser branches.
- New thread creation creates the folder and default `model.ecky` immediately.
- Every settled Ecky, agent, or external file save appends one immutable version,
  including versions whose validation or render fails.
- Validation/render outcomes attach to versions; they do not decide retention.
- Head always points to the last appended version. Successful versions remain
  separately filterable; failed versions are never dropped.
- Source file is shared working copy; SQLite is durable Ecky history. Git is
  optional and user-controlled.
- Existing `config.projectsRoot` is the default root and becomes visible in
  Settings. Bindings remain per-thread, not per-workspace.

## Scope

- Persistent binding for new threads and safe backfill of existing threads.
- Immediate mirror and external-file watcher with lossless append semantics.
- Visible `OPEN FILE` / `REVEAL FOLDER`; exact path shown to user.
- Agent target context exposes source path; normal authoring edits that file
  and syncs it.
- Reduce public MCP authoring surface to source binding/status/sync/open.

## Out Of Scope

- Per-workspace SQLite, source migration, Git integration, TUI/editor plugin,
  multi-file project scanning, or deleting current history/artifacts.
- Removing backend preview/commit primitives before all callers migrate.

## Success

- New thread gets editable source immediately.
- User finds `OPEN FILE` without hunting in Params.
- External/agent file edits become versions; serialized appends preserve each
  changed save and never require a conflict or force decision.
- Existing global history and artifact paths remain valid; validation/render
  failure or concurrent writes never lose a version.
