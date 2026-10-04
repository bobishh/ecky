# Design: Workbench Responsiveness

## Decisions

- **Goal:** unrelated interaction remains responsive while CAD and persistence
  work run; expensive work may still take seconds.
- **Artifact model:** existing immutable versions and render snapshots; bounded
  diagnostic samples are evidence, never lifecycle authority.
- **Variables:** async execution ownership, geometry permit lifetime, database
  critical sections, watcher invalidation, viewer preparation/cache identity,
  frame invalidation, diagnostic bounds, and native test conditions.
- **Decision:** repair execution boundaries in measured increments, retaining
  the existing Rust controller and serialized geometry admission.
- **Rejected paths:** blanket mutex replacement, unbounded `spawn_blocking`, more
  CAD concurrency, whole-app rewrite, and frontend scheduling authority.
- **Proof plan:** deterministic barrier tests for isolation/order/cancellation;
  native timings for perceived latency; focused browser tests for UI projection.

## Measurement before optimization

Extend existing projection observations rather than install another telemetry
platform. Capture command identity, exact version/digest when applicable, queue
wait, lock wait/hold, execution duration, watcher work counts, viewer preparation,
and input-to-next-painted-feedback. Add a runtime heartbeat and a webview frame
gap probe; neither changes application behavior or writes authoring history.

Diagnostics are opt-in, bounded to a 2048-sample in-memory ring, and exclude
source, prompts, terminal output, and credentials. Export only by explicit
diagnostic action. No new status bar or live agent terminal output in app logs.
Keep timing collection cheaper than the work being measured, and measure actual
guard release rather than labeling a timestamp before serialization as full hold.

Initial acceptance targets, to be recorded on a named reference machine/build:

| Measurement | Target |
| --- | --- |
| Click/typing to painted local feedback | p95 <= 100 ms |
| Lightweight read or stop acknowledgement under one render | p95 <= 200 ms |
| Longest interaction stall in the acceptance run | < 500 ms |
| Executor heartbeat gap with an injected blocked geometry job | < 100 ms |

These are proposed gates, not observed results. Gather at least 30 samples for
each interaction in idle, active render, watcher edit, and dense-version-switch
conditions after warmup; report p50/p95/max, scene size, source size, project
count, build mode, hardware, and profiling overhead. Keep cold-start separate.
Persist the baseline and after results in `verification.md` during implementation.

## Rust execution boundary

Keep render singleflight before geometry admission. Existing Rust lifecycle
services submit an owned job snapshot to a bounded worker and asynchronously
await its result. The job owns the geometry permit for the entire execution;
dropping a caller cannot release it while a non-cooperative worker still runs.
Keep explicit BUILD ordering and latest-pending interactive coalescing exactly
as specified by `exploration-build-cycle`; no additional work queue authority.

Preserve large-stack geometry helpers inside the worker if needed. Never call a
blocking thread join on an async executor worker. Cover capability/cache reads,
postprocessing, and artifact persistence as well as kernel execution. Cancellation
uses the existing flags/process controls; awaiting-task cancellation alone is not
worker termination. Publish completion/error to all singleflight subscribers and
retain evidence on obsolete versions without replacing the selected viewport.

Database connections execute on bounded blocking owners. Preserve one ordered
writer and the existing reader before considering a pool. Obtain owned projection
data, release the connection, then serialize/emit. Lock scopes contain database
work only, not filesystem reads, rendering, provider waits, or event emission.
Snapshot/read/revalidate is required when moving binding filesystem checks out of
a transaction: a changed binding must invalidate the repair, not overwrite it.

Move canonical config transactions and PTY writes to bounded blocking execution
as separate slices. Preserve config write ordering, atomic publication, raw error
behavior, and PTY byte order. Do not bypass `config_store::save_config` semantics.

## Watcher work

The notification transport retains normalized dirty paths in a bounded coalescing
set; overflow marks reconciliation necessary. Changes to project roots/bindings
update subscriptions. Targeted checks use the same append/apply services as today.
Fallback reconciliation scans in bounded batches and verifies content when needed;
metadata alone must not miss same-size rewrites or atomic file replacement.

No disk read or hashing occurs under the writer database mutex. Clean ticks avoid
redundant writes. Repeated notifications, missed notifications, inactive projects,
binding changes, and restart retain existing semantics. Preserve the existing
800 ms settle policy in the first slice and the two-second settled-edit apply
deadline; render duration remains separate. Test fallback coverage at the maximum
supported fixture inventory rather than assuming a slower poll is acceptable.

## Viewer preparation and frames

Read/parse and prepare dense geometry in a web worker with transferable typed
arrays; keep Three.js scene and GPU ownership in the webview. Cache prepared
arrays by immutable artifact digest plus display policy, with an explicit byte
budget and eviction. Preserve authored semantics and export geometry exactly.
Build optional wireframe only when requested. Bound scene attachment batches and
discard/dispose stale results using selected version/artifact and load identity.

Render on invalidation while continuing frames for active controls damping,
animation, and capture. Dirty sources include model attach, camera, resize,
selection, all overlays, lighting, theme, and visibility restoration. Screenshots
must await an up-to-date frame. Hidden/minimized views suspend unnecessary work.
The Ecky character's animation is independently budgeted if profiling implicates it.

## Validation boundaries and rollout

Use controlled blocked workers/storage seams to prove heartbeat, lightweight
commands, cancellation acknowledgement, and permit retention independently of
machine speed. Assert that a second render cannot enter until the actual first
job exits, including dropped awaiters and errors. Keep all existing version and
restart semantics. Run each focused Rust test as current compile proof; do not
duplicate it with `cargo check`.

Browser tests cover dense loading plus typing, model swaps, overlay visibility,
stale-result disposal, damping, and screenshot freshness. Native validation is
mandatory for the latency gates: current Vite/Node/mocked-invoke Playwright flows
do not execute the desktop Rust boundary. Follow the repository's outer red test,
inner unit red/green/refactor, and outer green sequence for every behavior slice.

Ship instrumentation first, then render isolation, watcher/DB scope, viewer
preparation, and frame invalidation. Reorder only when the recorded stalled path
justifies it. Keep relevant active specs/tasks synchronized as implementation
lands; this proposal does not mark their pending kernel work complete.
