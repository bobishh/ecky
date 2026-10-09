## Decision

Reuse `search_extracted_components` with an unfiltered header-only scan in the
existing `loadComponents` intent. Keep installed package headers as a separate
projection field; do not fabricate package coordinates for unversioned sources.

The workbench displays component summaries directly, with local/built-in origin,
optional pinned version, description, parameter keys, tags, and port summaries.
Search is a frontend projection of loaded summaries. Refresh reuses the load
intent. Existing installed-package import remains unchanged.

Successful persisted render outcomes pass authored Ecky source to the shared
component capture helper. It indexes only top-level `define-component` forms;
ordinary `part` forms are never promoted. Compiler free-variable analysis builds
the transitive closure of local `define` and `define-component` dependencies.
Dependencies are emitted before their dependents. If a definition has a source
dependency that cannot be resolved from its authored source, capture reports an
indexing diagnostic and leaves that component unavailable for reuse.

Automatic identity is an opaque digest of `(threadId, component name)`. Thread
IDs are project-scoped in current storage, so same-named components in separate
threads cannot overwrite each other. Each revision is stored below
`component-library/local-captures/<componentId>/revisions/<semanticDigest>/` with
copy-inline source and a schema-versioned header. The explicit `latest.json`
pointer changes atomically only after the immutable revision exists. Canonical
AST serialization removes comments and whitespace from revision identity;
component defaults, body, ports, verification clauses, and transitive local
definitions remain digest inputs. Instantiation parameters and placement live
outside the definition closure and do not create revisions. Removing a source
definition never deletes a stored revision.

The shared backfill helper indexes the latest successful, rendered source for
each extant thread on both Library load and MCP `component_search`. This indexes
existing work and catches successful writes from older paths without appending
or changing model versions. The DB guard covers latest-target selection and
pointer publication; each writer rechecks that its version is still latest.
Capture diagnostics remain visible in the Components view and MCP search result.
A stable `componentId` plus exact `revisionDigest` resolves any retained
revision; omitting the digest resolves the explicit latest pointer. This is
local storage and transport groundwork, not a network registry, account system,
or CLI command.

The package chooser accepts ZIP archives. The backend keeps reading legacy `.ecky`
package archives for compatibility; `.ecky` source files are not package input.

## Boundaries

Listing never reads or renders source bodies. Rust owns storage discovery and
camelCase translation. Frontend owns filtering and display only. Storage errors
remain actionable, with the existing retry surface.

## Proof

Backend acceptance: saved header without source body or package indexes appears
beside a built-in header; invalid storage location returns an error.
Browser acceptance: both origins appear without installed packages; parameter
search and no-match state work; existing error/retry and package import remain.
