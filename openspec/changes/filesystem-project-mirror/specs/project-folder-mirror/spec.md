## ADDED Requirements

### Requirement: Project Folder Export

The system SHALL export one thread's active macro to a plain filesystem folder
under a configurable projects root (`config.projectsRoot`, default
`<app_data>/projects`), writing `model.ecky` (the active macro source) and a
camelCase `ecky-project.json` manifest that binds the folder to its
`threadId`/`messageId`/`modelId` with a `sourceDigest` of the exported text.
Re-exporting the same project SHALL preserve the existing `projectId` so
external references stay valid.

#### Scenario: Export writes source and manifest

- GIVEN a thread with an active version
- WHEN the project is exported to a folder
- THEN the folder contains `model.ecky` with the active macro source
- AND the folder contains `ecky-project.json` binding threadId, messageId, and a
  sourceDigest of the written source

#### Scenario: Re-export keeps the project identity

- GIVEN a previously exported project folder
- WHEN the project is exported again
- THEN the manifest retains the original `projectId`

### Requirement: Digest-Based Sync Status

The system SHALL classify a project folder as `clean`, `fileChanged`,
`threadAdvanced`, `conflict`, or `missing` by comparing the file digest against
the manifest `sourceDigest` and the thread head against the bound `messageId`,
and SHALL do so without mutating the folder, the thread, or any history.

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

#### Scenario: Both sides moved is reported as conflict

- GIVEN an exported folder edited externally
- AND the thread advanced past the bound message
- WHEN status is requested
- THEN the folder is classified `conflict`

### Requirement: Project Folder Apply

The system SHALL apply an externally edited `model.ecky` by compile-checking the
file, rendering a preview, and committing it as a new version on the bound
thread through the existing preview/commit pipeline, then refreshing the
manifest. Apply SHALL refuse when the thread advanced past the manifest binding
unless the caller passes an explicit force flag, and SHALL never silently
clobber either side; on refusal the previous head remains available as a
version.

#### Scenario: Apply commits a new version

- GIVEN a folder classified `fileChanged`
- WHEN apply runs
- THEN the edited source is compiled and a preview is rendered
- AND a new version is committed on the bound thread
- AND the manifest is rebased onto the new head

#### Scenario: Stale folder refuses without force

- GIVEN a folder classified `conflict` or `threadAdvanced`
- WHEN apply runs without a force flag
- THEN apply refuses and reports why
- AND the existing thread head is left unchanged

### Requirement: Mirror Stays Out of the Database

The system SHALL treat the folder as a mirror only: all version writes go
through the existing preview/commit handlers, and no project-folder operation
writes the application database directly.

#### Scenario: Version writes flow through commit handlers

- GIVEN a project-folder apply
- WHEN the new version is persisted
- THEN it is written through the existing commit-preview handler
- AND no direct database write is performed by the mirror code
