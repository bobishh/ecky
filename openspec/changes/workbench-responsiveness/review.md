# Architecture Review — 2026-09-29

Scope: current working tree, including pre-existing uncommitted work. This is a
bounded source review, not a runtime profile. Line anchors describe the reviewed
snapshot and may move. Priorities indicate repair order, not measured cost.

## P1: Render work blocks an async executor worker

`src-tauri/src/services/render.rs:1965` enters an async render service, acquires
the geometry gate at 2002, and directly calls synchronous `render_model_unlocked`
at 2007. Its large-stack helpers spawn OS threads and immediately call `join()`
at 738, 761, and 833. Spawning the geometry thread therefore does not release the
async caller while that work executes. The call also covers synchronous cache,
capability, and artifact operations.

Consequence: a render occupies an executor worker for its duration; several
other synchronous paths can amplify executor starvation and delay stop/status
commands. This is not proof that a single render freezes all runtime workers.
The global render mutex intentionally serializes geometry; deleting it would
not fix the synchronous wait and could violate existing safety/ownership rules.

Repair: await a bounded worker completion, retaining existing admission and
singleflight. The executing job must own its permit until work really stops,
including when an awaiting caller disappears. Preserve stack sizes, errors,
cancellation, immutable evidence, and latest-eligible viewport publication.

## P1: One-second watcher work scales with project inventory

`src-tauri/src/mcp/handlers/project_folder.rs:17` defines an 800 ms settle debounce
and a one-second fallback. The transport at 620 receives filesystem events but
reduces them to a unit wakeup. `tick` at 905 lists project folders and bindings;
at 923–960 it holds the writer database mutex while inspecting provider folders
and reading their manifests when a binding differs. At 980–1017 it reads folder
manifests and rereads/hashes each active source. Even a clean source reaches
`clear_project_folder_watch_failure` at 1019, which takes the writer mutex at 839.

Consequence: periodic disk, hashing, and database work can cause rhythmic load;
slow storage in binding repair prolongs contention with prompt/version writes.
The 800 ms settle delay explains delayed external-file application, not delayed
ordinary clicks. It is presently allowed by the project-sync contract.

Repair: carry/coalesce dirty paths, snapshot bindings before disk I/O, validate
binding identity before a short repair transaction, and bound reconciliation.
Skip unchanged source reads and redundant failure clearing where safe. Preserve
missed-event recovery and settled-edit deadlines, including same-size rewrites.

## P1: Dense geometry preparation runs on the webview thread

`src/lib/Viewer.svelte:1450` loads multipart assets and immediately calls
`prepareLoadedAssetMeshes` (1530). That traversal prepares display geometry,
computes bounds, and builds both outlines (`EdgesGeometry`, 1805) and topology
(`WireframeGeometry`, 1829). Topology is created even though its initial opacity
is zero. These synchronous calls block input while preparing dense geometry;
awaiting a loader does not move the following preparation to another thread.

Repair: worker-based typed-array preparation, identity-keyed reuse, lazy optional
overlays, and a bounded main-thread attach step. Stale preparation must not attach
to a newly selected version. Preserve authored selection targets and exact export
geometry; visual simplification is not part of this proposal.

## P2: Blocking database and peripheral I/O share async execution

`src-tauri/src/commands/history.rs:17`, `:119`, and `:401` await a connection lock
and execute synchronous history queries/projection building on the async worker.
All preferred reads still share one reader mutex (`models.rs:474`). The database
initialization at `db.rs:1396` configures a 5000 ms busy timeout; contention may
therefore sleep inside a synchronous database call. This is an upper bound, not
evidence that normal reads take five seconds.

Other concrete examples: `commands/config.rs:144` calls config persistence before
its `spawn_blocking` supervisor sync; `config_store.rs:394` holds a process mutex
and retries a file lock with 10 ms sleeps (file-lock timeout 250 ms). PTY input
at `commands/session.rs:177` performs `write_all`/`flush` under a synchronous
writer mutex directly inside an async command.

Repair: execute these operations on bounded blocking owners with short scopes.
Keep canonical config persistence and writer transaction order. Do not replace
an async mutex with a synchronous mutex on an executor thread. Measure before
adding reader pools. Bound PTY backpressure without dropping accepted keystrokes.

## P2: Stationary viewport renders continuously

`src/lib/Viewer.svelte:1194` unconditionally schedules another frame, updates
controls, and renders the full scene. Dense scenes incur repeated work even with
no input or scene change. This is a background CPU/GPU cost, not measured proof
of a one-second event-loop stall.

Repair: invalidate on camera/scene/resize/selection changes, keep frames alive
while damping/animation needs them, suspend invisible views, and force a current
frame for screenshots. Profile Ecky's separate animation only if it remains hot.

## Safeguards found / suspects not established

- History has a separate reader and bounded projections; this is not a proposal
  to add those features again. The projection profiler already records selected
  waits/holds, but is debug-only and does not cover click-to-paint.
- Render singleflight already joins identical requests before the geometry gate.
- Codex enqueue explicitly wakes the supervisor (`commands/codex_takeover.rs:1173`)
  and terminal notifications wake it too (`services/codex_app_server.rs:714`).
  Both provider supervisors select a wake signal alongside their one-second
  fallback. A timer constant alone does not prove one-second prompt dispatch.
- The 250 ms folder-activity UI poll has an in-flight guard in
  `App.svelte:984`; it is not an overlapping request storm by construction.
- Capture's one-second poll and the display clock are not proof that all input
  is throttled. Capture polling deserves a separate overlap/visibility check if
  the native trace implicates it.
- No circular lock dependency/deadlock was established in this review.
- `playwright.config.ts` launches Vite and the Node server; for example,
  `e2e/params.spec.ts:95` mocks Tauri invoke. Those browser tests cannot establish
  real Rust executor or native webview responsiveness.

## First experiment

Record idle, one active render, one external source edit, and dense version switch
on the same native app build. Capture input handler/next paint, command request/
acknowledgement, executor heartbeat, render wait/execution, database wait/hold,
watcher bytes/files, and viewer preparation duration. Correlate the actual stalled
interaction before changing queue cadence or increasing worker counts.
