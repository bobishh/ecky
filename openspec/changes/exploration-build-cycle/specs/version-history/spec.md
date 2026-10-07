## ADDED Requirements

### Requirement: Exploration references normal immutable versions

Exploration cycle state SHALL reference versions created by the normal lossless
append path. It SHALL NOT redefine version creation, head selection, status, success
filtering, or result attachment.

#### Scenario: Cycle build follows normal append semantics

- **GIVEN** cycle BUILD changes version-owned source
- **WHEN** the source is persisted
- **THEN** normal version append creates exactly one immutable version before checks
- **AND** normal head advances to that append independent of outcome
- **AND** exploration stores the returned version ref.

#### Scenario: Cycle selection is not promotion

- **GIVEN** a cycle compares versions A and B
- **WHEN** DECIDE records B as the chosen cycle result
- **THEN** A and B retain their identities, statuses, evidence, and append order
- **AND** no extra version is created.

### Requirement: Cycle grouping does not hide failed versions

The system SHALL preserve all exploration-created versions in normal history. Cycle
grouping and successful/printable filters SHALL be projections only.

#### Scenario: Failed exploration remains addressable

- **GIVEN** versions B and C were created during one cycle and both failed
- **WHEN** the cycle stops
- **THEN** B and C remain addressable in version history with exact source and errors
- **AND** stopping the cycle does not discard, squash, or reclassify them.

### Requirement: Version and viewport heads remain distinct projections

Exploration orchestration SHALL preserve latest-append version head semantics while
allowing the active viewport to retain the newest eligible successful render.

#### Scenario: New red version does not erase last good render

- **GIVEN** version A has a successful render and is visible
- **WHEN** newer version B is appended and render fails
- **THEN** version head is B
- **AND** viewport may continue showing A
- **AND** UI and cycle context identify both refs explicitly.

### Requirement: Successful renders retain durable version thumbnails

The shared Rust persistence path SHALL create a geometry-derived PNG for each
attached successful render before runtime STL cleanup. Thumbnail generation SHALL
not depend on active thread selection, viewport visibility, or frontend load events.
A thumbnail SHALL belong to its exact version runtime; attaching a changed runtime
SHALL replace an obsolete thumbnail. Existing matching viewport images MAY be retained.
Project cards SHALL show the newest available rendered version thumbnail when newer
versions are pending, failed, or lack an image. Missing legacy thumbnails SHALL be
backfilled from available persisted STL without rendering CAD or creating a version.

#### Scenario: Background render survives restart and newer failure

- **GIVEN** two versions render without being loaded into the visible viewport
- **WHEN** a newer version fails and the app restarts
- **THEN** both rendered versions retain their own PNG
- **AND** the project card still displays the latest available rendered thumbnail.

#### Scenario: Legacy preview is recovered without rewriting geometry

- **GIVEN** a historical version lacks a thumbnail but retains its STL
- **WHEN** its project preview is requested or its STL becomes eligible for cleanup
- **THEN** Rust derives and persists a PNG from that exact STL
- **AND** creates no new immutable version and performs no CAD rebuild.
