## ADDED Requirements

### Requirement: Every Thread Has a Persistent Editable Source Binding

The system SHALL preserve global Ecky SQLite history and SHALL bind every new
thread to a persistent source folder and `model.ecky` under configured source
root. It SHALL create folder, default source, and sidecar binding manifest on
thread creation. It SHALL retain exact bound path after title/root changes.
The `stable-project-folder-names` startup migration MAY replace exact legacy
`untitled-design-<id>` placeholders with neutral names before source consumers
start; it SHALL preserve contents and update stored source/provider paths together.

#### Scenario: New thread gets source immediately

- GIVEN a configured source root
- WHEN user creates blank thread
- THEN thread folder and `model.ecky` exist under root
- AND `history.sqlite` contains exact source binding
- AND file contains default editable source

#### Scenario: Macro thread is created through one backend intent

- GIVEN user supplies a title and non-empty initial macro source
- WHEN workbench creates a macro thread
- THEN Rust allocates the thread and initial-version identities
- AND one intent creates the source binding, appends the exact source, validates,
  renders, and attaches runtime or raw failure evidence
- AND response contains the created bound source and bounded workspace projection
- AND frontend does not compose blank creation with a later manual commit
- AND new-project, edited-source fork, and detached manual-commit callers do not
  manufacture thread identities

#### Scenario: Edited-source fork retains immutable base policy

- GIVEN edited source and an exact base thread/version identity
- WHEN workbench creates the fork
- THEN Rust derives language, geometry backend, parameters, controls, and
  post-processing from the persisted base version
- AND partial or stale base identity fails before thread, binding, folder, or
  version mutation

#### Scenario: Invalid creation intent leaves no partial thread

- GIVEN macro mode has no non-empty source or blank mode carries source
- WHEN creation intent is validated
- THEN it fails before thread, binding, folder, or version mutation.

### Requirement: Ecky and External Editors Synchronize One Bound File

The system SHALL append one immutable version for every settled changed save
from Ecky, an agent, or an external editor, regardless of validation/render
outcome. Each version SHALL retain source bytes (or a durable source
reference/digest) plus validation and render status, including raw failure
details when available. Append order SHALL advance head to the last appended
version. It SHALL not create a version from an unchanged observation.

#### Scenario: External save becomes version

- GIVEN bound thread with saved version
- WHEN external editor saves valid changed bound source
- THEN watcher appends exactly one immutable version
- AND the version is marked valid by validation/render status
- AND Ecky leaves external source bytes unchanged

### Requirement: Source Saves Are Lossless And Conflict-Free

The system SHALL serialize Ecky/agent writes and external saves through one
append boundary. A digest mismatch SHALL NOT be treated as a conflict and
SHALL NOT cause refusal, rollback, history loss, or a force-overwrite branch.
The last append SHALL become head. External source bytes SHALL remain intact
for the external append. Version validation/render status SHALL be filterable
without removing failed versions from history.

#### Scenario: Invalid external save remains history

- GIVEN bound thread whose source has changed since the previous head
- WHEN the external save settles with invalid source
- THEN one immutable version appends
- AND its validation/render status records failure
- AND head points to that version
- AND no conflict or force decision is required

#### Scenario: Concurrent saves append serially

- GIVEN Ecky and an external editor save changed source near the same time
- WHEN both append operations complete
- THEN both immutable versions remain in history in append order
- AND head points to the last appended version
- AND neither operation is rejected as a conflict

### Requirement: Source Actions Are Discoverable

The system SHALL show `OPEN FILE` and `REVEAL FOLDER` for bound active thread
in Projects, label Params action `OPEN FILE`, and show raw editor/filesystem
failure instead of only logging it.

#### Scenario: Open source from Projects

- GIVEN active bound thread
- WHEN user opens Projects
- THEN OPEN FILE and REVEAL FOLDER are visible with bound path
- AND OPEN FILE opens that exact source file with OS-configured editor

### Requirement: Agents Receive and Use Bound Source Path

The system SHALL expose bound source path, folder, and sync state in agent
target metadata. Normal agent authoring SHALL edit that file and use source
sync, not export a mirror or use public macro mutation tools.

#### Scenario: Agent edits thread source

- GIVEN agent attached to bound thread
- WHEN it reads metadata and edits supplied source path
- THEN source sync validates and commits edit as new version
- AND no export operation is required
