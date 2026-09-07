## ADDED Requirements

### Requirement: Project Folder Export

The system SHALL export one thread's active macro to a plain filesystem folder
under a configurable projects root (`config.projectsRoot`, default
`<app_data>/projects`), writing `model.ecky` (the active macro source) and a
kebab-case `ecky-project.edn` manifest that binds the folder to its
`thread-id`/`message-id`/`model-id` with a `source-digest` of the exported text.
Re-exporting the same project SHALL preserve the existing `project-id` so
external references stay valid.

#### Scenario: Export writes source and manifest

- GIVEN a thread with an active version
- WHEN the project is exported to a folder
- THEN the folder contains `model.ecky` with the active macro source
- AND the folder contains `ecky-project.edn` binding thread-id, message-id, and
  source-digest of the written source

#### Scenario: Re-export keeps the project identity

- GIVEN a previously exported project folder
- WHEN the project is exported again
- THEN the manifest retains the original `project-id`

### Requirement: Digest-Based Sync Status

The system SHALL classify a project folder as `clean`, `fileChanged`, or
`missing`, and MAY report head movement as an informational flag. Classification
SHALL compare file digest against manifest `source-digest` and SHALL not create
a conflict gate or mutate history.

#### Scenario: Unedited folder on the bound head is clean

- GIVEN an exported folder whose `model.ecky` matches the manifest digest
- AND the thread head is still the bound message
- WHEN status is requested
- THEN the folder is classified `clean`
- AND nothing on disk or in history is modified

#### Scenario: External edit is reported as fileChanged

- GIVEN an exported folder whose `model.ecky` no longer matches the manifest
  digest
- AND the thread head is still the bound message
- WHEN status is requested
- THEN the folder is classified `fileChanged`

#### Scenario: Both sides moved remains applicable

- GIVEN an exported folder edited externally
- AND the thread advanced past the bound message
- WHEN status is requested
- THEN status reports `fileChanged` with head movement
- AND apply remains allowed

### Requirement: Project Folder Apply

The system SHALL append every externally changed `model.ecky` as a new version
before compile-checking or rendering it. That appended version SHALL become
head regardless of validation/render outcome, retain exact source bytes, and be
persisted through the existing preview/commit pipeline. Successful versions
SHALL be separately filterable. Unchanged content SHALL be idempotent.

#### Scenario: Apply commits a new version

- GIVEN a folder classified `fileChanged`
- WHEN apply runs
- THEN a new version containing exact edited source is appended on the bound
  thread and becomes head
- AND that version is compiled and previewed
- AND the manifest is rebased onto the new head

#### Scenario: Invalid source still becomes head

- GIVEN a changed folder containing invalid source
- WHEN apply runs
- THEN a new version is appended before compilation
- AND that failed version is the thread head
- AND the raw validation error is retained on the version

#### Scenario: Both sides changed serialize without conflict

- GIVEN the thread advanced and the folder file changed
- WHEN apply runs
- THEN the file is appended as the newest version
- AND the prior head remains available in history

#### Scenario: Unchanged content is idempotent

- GIVEN the folder source matches the current head source exactly
- WHEN apply runs
- THEN no duplicate version is created

#### Scenario: Successful versions filter independently

- GIVEN history contains successful and failed appended versions
- WHEN successful versions are requested
- THEN failed versions are excluded

### Requirement: Mirror Stays Out of the Database

The system SHALL treat the folder as a mirror only: all version writes go
through the existing preview/commit handlers, and no project-folder operation
writes the application database directly.

#### Scenario: Version writes flow through commit handlers

- GIVEN a project-folder apply
- WHEN the new version is persisted
- THEN it is written through the existing commit-preview handler
- AND no direct database write is performed by the mirror code

### Requirement: Watcher eligibility follows authoring ownership

The watcher SHALL derive eligible threads from active authoring surfaces rather
than rendered-version state. Selected UI threads and live MCP session targets
SHALL be eligible. A thread SHALL NOT need an existing version or render
snapshot before its changed bound source can be appended.

#### Scenario: MCP-owned blank thread creates its first version

- GIVEN a live MCP session owns a bound blank thread with no versions
- WHEN the session changes that thread's `model.ecky`
- THEN the watcher appends the exact source as the thread's first immutable version
- AND normal validation, render, verification, and failure persistence run
- AND no synthetic seed version or no-version recovery branch is used

#### Scenario: Inactive folder remains isolated

- GIVEN a bound folder belongs to neither the selected UI thread nor a live MCP target
- WHEN its source differs from the manifest
- THEN the watcher leaves it pending and does not append a version
