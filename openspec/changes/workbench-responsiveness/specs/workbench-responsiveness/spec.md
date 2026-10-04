## ADDED Requirements

### Requirement: Blocking work is isolated from interactive async execution

The application SHALL execute blocking geometry, database, filesystem-lock, and
PTY work through bounded blocking execution boundaries. Lightweight reads and
stop acknowledgement SHALL NOT wait for unrelated geometry completion. Rust SHALL
retain existing lifecycle and scheduling authority.

#### Scenario: Render is blocked while a user stops work

- **GIVEN** a geometry worker is held at a controlled execution barrier
- **WHEN** a lightweight state read and a stop intent arrive
- **THEN** both are serviced before that barrier is released
- **AND** an async heartbeat continues
- **AND** stop acknowledgement does not claim the worker has already exited.

### Requirement: Geometry admission lasts until execution terminates

The executing job SHALL own its geometry permit until actual worker termination.
Dropping an awaiting caller SHALL NOT admit overlapping geometry. Existing
singleflight, immutable evidence, and latest-eligible publication rules SHALL hold.

#### Scenario: Caller disappears during synchronous execution

- **GIVEN** job A owns the geometry permit and still executes
- **WHEN** its caller is dropped and job B is admitted for later execution
- **THEN** B cannot enter geometry until A exits
- **AND** remaining singleflight subscribers receive a defined result or raw error
- **AND** an obsolete result cannot replace the selected viewport.

### Requirement: Shared database locks exclude external work

Database critical sections SHALL exclude filesystem inspection, hashing, provider
waits, geometry execution, and event emission. Binding repairs SHALL revalidate
identity after external inspection before applying a short ordered transaction.

#### Scenario: Slow manifest storage does not monopolize database writes

- **GIVEN** watcher manifest inspection is suspended at a storage barrier
- **WHEN** an unrelated prompt is enqueued through the Rust controller
- **THEN** its database transaction completes before storage is released
- **AND** a binding changed during inspection is not overwritten by stale repair.

### Requirement: Watcher reconciliation is bounded and preserves recovery

The watcher SHALL coalesce dirty-path notifications and bound fallback work.
Unchanged sources SHALL avoid redundant full reads and failure-state writes on
ordinary clean ticks. Missed events SHALL remain recoverable, including same-size
rewrites and atomic replacements. Existing settled-edit apply deadlines SHALL hold.

#### Scenario: Large clean inventory does not create a recurring full scan

- **GIVEN** the supported project-inventory fixture is settled and unchanged
- **WHEN** fallback ticks run without source notifications
- **THEN** work per tick stays within the recorded reconciliation budget
- **AND** active source contents are not all reread on every one-second tick.

#### Scenario: A notification is lost

- **GIVEN** a source changes and its notification is omitted
- **WHEN** fallback reconciliation reaches that source
- **THEN** the normal guarded apply detects its exact changed content
- **AND** exactly one version is appended within the existing apply deadline.

### Requirement: Dense viewer preparation preserves interactive input

Dense asset decoding and CPU geometry preparation SHALL execute outside the
webview thread. Scene attachment SHALL be bounded, optional topology SHALL be
lazy, and cached results SHALL be identified by immutable artifact and policy.

#### Scenario: User types while a dense version loads

- **GIVEN** dense geometry preparation is in progress
- **WHEN** the user types and then selects another version
- **THEN** local feedback remains within the recorded responsiveness gates
- **AND** stale prepared geometry cannot attach to the new selection
- **AND** authoritative geometry and authored selection identities are preserved.

### Requirement: Stationary rendering is driven by visual changes

The viewer SHALL render on invalidation and continue frames only while interaction,
damping, animation, or capture needs them. Visibility restoration and every visual
state change SHALL invalidate the frame. Screenshot evidence SHALL use current state.

#### Scenario: Idle view resumes for selection and capture

- **GIVEN** the scene and camera are stationary with no active animation
- **WHEN** a selection changes and a screenshot is requested
- **THEN** a fresh frame includes the selection before capture resolves
- **AND** continuous rendering stops again after visual motion settles.

### Requirement: Responsiveness claims have bounded native evidence

Diagnostics SHALL separate input/paint, command acknowledgement, executor lag,
lock wait/hold, and worker duration. Samples SHALL be bounded and exclude user
content and secrets. Acceptance SHALL use the named native build, fixtures, and
latency targets in this design; mocked browser IPC SHALL NOT count as Rust proof.

#### Scenario: A repair is declared complete

- **GIVEN** focused isolation and lifecycle regression tests pass
- **WHEN** native acceptance results are recorded
- **THEN** idle, rendering, watcher, and dense-load scenarios include p50/p95/max,
  sample count, hardware/build, workload sizes, and profiling overhead
- **AND** unmet targets remain explicit unfinished tasks
- **AND** no new authoring version is created solely for diagnostics.
