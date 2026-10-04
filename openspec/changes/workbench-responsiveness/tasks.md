# Tasks: Workbench Responsiveness

## 0. Review and contract

- [x] Trace relevant source paths and distinguish confirmed blocking from unmeasured impact.
- [x] Record evidence, proposed boundaries, existing safeguards, and spec relationships.
- [x] Validate this change with strict OpenSpec validation.

## 1. Baseline and failing acceptance

- [ ] Record the reported stalled interaction on the native app; capture idle,
  render, external-edit, and dense-version-switch baselines in `verification.md`.
- [ ] Record fixture sizes, native build/hardware, sample counts, p50/p95/max,
  and profiling overhead against design targets.
- [ ] Add a failing Rust integration test: a controlled blocked render does not
  prevent heartbeat, lightweight reads, or stop acknowledgement; confirm the
  failure is executor blocking, not fixture/setup failure.
- [ ] Add bounded opt-in timing evidence using unit red/green cycles; retain no
  source, prompt, credentials, or terminal content.

## 2. Render isolation

- [ ] Add failing unit tests for worker completion, raw error/panic propagation,
  caller disappearance, cancellation, permit lifetime, and singleflight waiters.
- [ ] Move the complete blocking render path behind bounded execution, preserving
  large stack sizes and the existing Rust controller/admission services.
- [ ] Prove a second job never enters before the real first worker exits.
- [ ] Turn the integration test green; verify explicit BUILD ordering,
  latest-pending coalescing, obsolete evidence, and no stale viewport publication.

## 3. Watcher and persistence isolation

- [ ] Add failing integration coverage for slow manifest storage concurrent with
  prompt enqueue, and stale binding repair during an external inspection.
- [ ] Drive short database scopes and blocking DB ownership through unit tests;
  preserve ordered writes, bounded history reads, and canonical config behavior.
- [ ] Add failing watcher integration coverage for large clean inventory, duplicate
  events, same-size edits, atomic replacement, missed events, and changed roots.
- [ ] Implement dirty-path coalescing, bounded overflow/reconciliation, and safe
  unchanged-work suppression; preserve the two-second apply deadline.
- [ ] Separately drive config save and ordered PTY backpressure through failing
  acceptance/unit tests before moving their blocking work.

## 4. Viewer responsiveness

- [ ] Add a failing dense-load interaction test that exercises real preparation
  while typing/switching versions; confirm the expected input-delay failure.
- [ ] Unit-test worker result identity, transferable buffers, cache byte limits,
  eviction, disposal, and lazy topology before implementation.
- [ ] Implement worker preparation and bounded scene attachment; make the outer
  interaction test green without changing authoritative geometry.
- [ ] Add failing coverage for stationary render suppression, camera damping,
  overlays, resize/theme, hidden-view resume, and screenshot freshness.
- [ ] Implement invalidation rendering and pass those focused tests.

## 5. Completion gates

- [ ] Re-run native acceptance with the same fixtures and record after/before
  timings; do not substitute mocked Playwright invokes for Rust/native proof.
- [ ] Pass focused Rust and frontend regression suites for changed behavior,
  including immutable history, stop, restart, and stale-result rejection.
- [ ] Use successful relevant Rust tests as compile proof; avoid duplicate checks.
- [ ] Synchronize affected active specifications and tasks without marking unrelated
  kernel-performance work complete; run strict validation.
- [ ] Stop when the declared gates pass; do not stage or commit unless requested.
