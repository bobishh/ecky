# Design

## Naming

`project_mirror::project_slug` uses `ecky-design` for blank, punctuation-only,
or default `Untitled design` titles. A supplied title uses at most 40 Unicode
letters/digits with collapsed hyphens. The existing eight-character UUID suffix
keeps identity stable. Directory validation and stored-folder export use the
same Unicode-safe component policy. Paths never derive from later titles.

## Legacy migration boundary

Run after stale-provider recovery and before loading the selected source,
starting provider supervisors, MCP listeners, or file watchers. No online
directory relocation is introduced. This is a narrow exception to the original
thread-source-binding retained-path contract, limited to legacy placeholders.

Only a stored binding whose basename exactly matches
`untitled-design-<its-thread-suffix>` qualifies. Move to `ecky-design-<suffix>`
under the same parent. Require a regular directory, regular `model.ecky`, and
an ownership manifest with the expected thread identity. Preserve all bytes,
including dirty external edits, assets, dependency locks and the manifest.
Never copy history or regenerate source during relocation.
Unresolved provider process leases defer migration until their delivery is
resolved, even if startup attempted to reap an orphaned process.

Backfill discovers unindexed existing mirrors by their manifest thread identity
before creating source under the new naming rule. Ambiguous ownership fails
without rewriting either folder, preserving external edits in legacy mirrors.

## Recovery and collisions

Persist a SQLite relocation intent containing thread identity, manifest project
identity and both paths before moving anything. Use an exclusive atomic rename
that refuses even empty existing destinations. Sync the parent directory on
Unix. Then update the source binding and matching Codex/Agy stored working
directories in one transaction and remove the intent. Reset their bootstrap
versions so the next resume refreshes path-bearing instructions; retain native
conversation IDs, queued prompts and history.

On restart, recover pending intents before considering other legacy folders.
The manifest identifies the moved directory. Ambiguous, foreign, symlinked, or
missing directories produce actionable errors without overwriting content.
An interrupted move whose binding is still stale blocks application startup
until recovery succeeds, preventing consumers from recreating an empty folder
at the old path. A collision before movement leaves the existing binding usable
and reports a warning in app logs.
