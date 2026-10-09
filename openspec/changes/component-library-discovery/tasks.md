## Tasks

- [x] Author backend and UI acceptance tests for saved/built-in discovery.
- [x] Confirm backend acceptance fails because component summaries are absent.
- [x] Extend existing Rust projection and generate boundary types.
- [x] Display and search available components, preserving package import.
- [x] Verify focused backend tests and UI happy/failure states.
- [x] Capture authored definitions after successful persisted renders and backfill
  the latest successful source per thread without appending model versions.
- [x] Preserve thread-scoped stable component IDs, immutable semantic revisions,
  transitive source closure, and pinned revision reads.
- [x] Surface unresolved dependency and history-indexing diagnostics; retain
  component revisions when source definitions disappear.
- [x] Expose stable componentId and exact revisionDigest in MCP component_get;
  backfill successful history before MCP component_search.
- [x] Restrict package chooser to ZIP while retaining legacy backend archive
  compatibility.
