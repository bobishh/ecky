## Decision

Reuse `search_extracted_components` with an unfiltered header-only scan in the
existing `loadComponents` intent. Keep installed package headers as a separate
projection field; do not fabricate package coordinates for unversioned sources.

The workbench displays component summaries directly, with local/built-in origin,
optional pinned version, description, parameter keys, tags, and port summaries.
Search is a frontend projection of precomputed lowercase summary text; typing
never reads source, backfills history, or invokes geometry. MCP header search
also includes parameter keys and port identifiers for every origin. Refresh reuses the load
intent. Existing installed-package import remains unchanged.

Successful persisted render outcomes pass authored Ecky source to the shared
component capture helper. It indexes top-level `define-component` forms and extracts ordinary named
`part`/`feature` forms into closed reusable definitions. Existing extraction
preserves referenced model parameter metadata and scalar lexical defaults. For
computed lexical geometry without ports, automatic capture retains the original
ordered let/let* frames around the geometry instead of evaluating them; local
top-level helper definitions are included transitively. Unsupported dependencies
produce per-part diagnostics without hiding independently reusable entries.
A part whose key already names a captured component reuses that definition. Compiler free-variable analysis builds
the transitive closure of local `define` and `define-component` dependencies.
Dependencies are emitted before their dependents. If a definition has a source
dependency that cannot be resolved from its authored source, capture reports an
indexing diagnostic and leaves that component unavailable for reuse.

Automatic identity is an opaque digest of `(threadId, component name)` for
authored definitions and `(threadId, "part", part key)` for extracted parts. Thread
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

A schema-versioned per-thread capture cursor records the indexed immutable
message ID and diagnostics. Repeated Library loads/MCP searches skip source
reads and extraction for unchanged messages; the cursor is published only after
revision writes. Changed successful messages are indexed again.

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

## Reuse and engine boundary

Every available Ecky card has INSERT INTO CODE. The request carries either a
stable local component ID plus exact revision digest, a built-in/extracted name,
or exact installed package coordinates. Rust reads authoritative bound source,
checks its expected digest, inserts closed definitions and one part, then uses
the existing manual-code service to persist and render one immutable version.
No frontend source splice or lifecycle orchestration is added. Identical existing
definitions are reused for repeated instances; differing definitions reject with
an actionable conflict. MCP component_import accepts the same selections and
returns source for the normal authoring flow. Local reuse never follows latest
implicitly and never creates a package dependency lock.

FreeCAD Parts is visible only when the active authoring backend is FreeCAD.
Its MCP tools remain explicitly described as the separate FreeCAD-engine catalog;
native Ecky discovery uses component_search/component_get/component_import.
Foreign donor/history/runtime support remains intact.

## Scoped editing assessment (deferred)

Do not infer editing safety from a threshold of two parts: source complexity and
shared parameter/helper dependencies do not track part count. A future scope
contract should bind selection to source digest and stable AST identity, preserve
unselected source, validate declared shared-dependency edits in Rust, and apply
the same enforcement to UI, MCP, and watched files. Whole-model refactoring needs
an explicit operation. This change repairs component discovery/reuse and does not
introduce that cross-cutting editing policy.
