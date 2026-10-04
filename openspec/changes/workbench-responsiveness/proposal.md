# Proposal: Workbench Responsiveness

## Why

The workbench intermittently feels as though input advances in one-second steps.
A source review found blocking render execution inside async services, periodic
project scans with disk I/O inside a database critical section, and eager mesh
preparation on the webview thread. These are concrete blocking paths, but their
contribution to the reported latency has not yet been measured.

## What Changes

- Establish separate measurements for input-to-paint, command acknowledgement,
  async executor lag, lock wait/hold, watcher work, and viewer preparation.
- Move blocking render execution behind a bounded execution boundary without
  changing Rust admission, immutable versions, singleflight, or publication rules.
- Remove filesystem work from database critical sections and move blocking
  database/config/PTY operations off async executor workers.
- Make watcher notifications carry dirty paths; bound fallback reconciliation
  instead of fully rereading every active source every second.
- Move dense viewer preparation to a worker and build optional topology lazily;
  render stationary views on invalidation while preserving interaction/capture.
- Prove responsiveness under background work through deterministic concurrency
  tests plus a native-app latency recording.

## Scope and Relationships

`exploration-build-cycle` remains authoritative for lifecycle, latest-wins,
cancellation, ASK, and restart behavior. `lossless-version-history` remains
authoritative for append-before-validation and exact result identity. This change
adds execution and responsiveness constraints, not another controller.

`hybrid-render-performance-job-control` owns kernel optimization, shared-job
cancellation, artifact reuse, and compact preview transport. Reuse those services
and coordinate overlapping tasks there; do not create a competing kernel actor.
`ast-derived-parameter-provenance/specs/project-sync-performance/spec.md` retains
the settled-edit apply deadline. `frontend-decomposition` remains a separate
structural cleanup; splitting files alone is not a performance fix.

## Out of Scope

- Parallel CAD execution, geometry simplification, renderer replacement, or a
  database migration undertaken without measurements.
- Frontend generation/retry/queue authority, new agent status UI, or changed
  version/head semantics.
- Claiming a deadlock or a measured one-second stall from static inspection.

## Deliverables and Proof

This proposal is file-backed Markdown in this directory: evidence in `review.md`,
execution decisions in `design.md`, normative additions in
`specs/workbench-responsiveness/spec.md`, and failure-first work in `tasks.md`.
There is no serving route or frontend documentation shell.

The proposal is complete when strict OpenSpec validation passes. Implementation
remains pending until the concurrency, native responsiveness, and lifecycle
regression gates in the design pass. Existing application behavior is unchanged
by this proposal.
