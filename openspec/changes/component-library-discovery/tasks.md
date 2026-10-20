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

- [x] Extend acceptance coverage for rendered named parts, pinned reuse, repeated
  insertion, source conflicts, pending/errors, and engine-specific foreign catalog.
- [x] Capture ordinary parts with transitive local helpers and per-part diagnostics.
- [x] Skip unchanged history with durable capture cursors; search header metadata.
- [x] Wire native card insertion and MCP reuse to the shared Rust source operation.
- [x] Verify focused Rust, browser, contract generation, and frontend type checks.
