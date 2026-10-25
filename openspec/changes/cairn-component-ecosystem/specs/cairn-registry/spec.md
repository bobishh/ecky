## ADDED Requirements

### Requirement: Versioned metadata and immutable downloads
The registry SHALL provide bounded search, per-package version metadata and digest-addressed package downloads through a versioned HTTP interface with conditional metadata caching.

#### Scenario: Unchanged metadata
- **GIVEN** a cached metadata ETag
- **WHEN** the client requests the unchanged resource conditionally
- **THEN** the registry reports it unchanged without retransmitting the full metadata body

#### Scenario: Large release history
- **GIVEN** a package with more than one metadata page
- **WHEN** a client installs an exact version
- **THEN** it reads that release directly without downloading other versions, while browsing returns bounded pages and continuation offsets

### Requirement: Authenticated immutable publication
The registry SHALL authenticate write operations, validate bounded uploads and enforce immutable package coordinates.

#### Scenario: Identical republish
- **GIVEN** an already published coordinate and identical artifact bytes
- **WHEN** an authorized client republishes it
- **THEN** publication is idempotent

#### Scenario: Conflicting republish
- **GIVEN** an already published coordinate
- **WHEN** an authorized client submits different bytes for it
- **THEN** the registry rejects the conflict and preserves the original release

#### Scenario: Missing write credential
- **GIVEN** an unauthenticated publication request
- **WHEN** the server handles it
- **THEN** no release/blob is made discoverable and no credential is exposed in errors or logs

### Requirement: Recoverable release publication
The registry SHALL verify and persist immutable bytes before transactional metadata publication so crashes cannot advertise incomplete releases.

#### Scenario: Interrupted publish
- **GIVEN** an interrupted upload or process exit before metadata commit
- **WHEN** the registry restarts
- **THEN** no incomplete release is offered for installation and existing releases remain downloadable

### Requirement: Yank without broken locks
The registry SHALL permit soft yanking while retaining immutable content required by existing exact locks.

#### Scenario: New selection versus locked install
- **GIVEN** a yanked release
- **WHEN** a client performs fresh discovery/selection
- **THEN** that release is excluded from default selection while an existing exact lock can still retrieve its retained bytes

### Requirement: Independent persistent container
The registry engine SHALL build and run as a standalone server binary without desktop or geometry runtime dependencies. Its portable container wrapper SHALL support Docker and Podman, non-root execution, an external data volume, health checks and a documented consistent backup/recovery path without container-engine APIs or privileged mode.

#### Scenario: Server restart
- **GIVEN** a deployed registry and persistent metadata/blob volume
- **WHEN** its container is replaced with the same schema-compatible revision
- **THEN** previously published releases remain searchable and installable

#### Scenario: Docker and Podman deployment
- **GIVEN** the same registry image, configuration and data volume
- **WHEN** the operator starts it with Docker or Podman
- **THEN** the same HTTP package service is available without engine-specific runtime dependencies

### Requirement: Deliberate public content
The system SHALL publish only explicitly selected package content; local model indexing SHALL NOT publish private source or history.

#### Scenario: Local model catalog recovery
- **GIVEN** private local model history and a configured public registry
- **WHEN** the local catalog is rebuilt
- **THEN** no local source or package is uploaded by that recovery operation


### Requirement: Branded public Hub
The registry SHALL serve a component/package browser at `hub.ecky-cad.com`, reusing Ecky's maintained landing theme and mascot directly. Source inspection SHALL be bounded text with package integrity and authored license metadata.

#### Scenario: Inspect and install a discovered component
- **GIVEN** an intentionally published package export
- **WHEN** a visitor searches and inspects it
- **THEN** the Hub shows its parameters, exact coordinate, bounded source and CLI installation command

#### Scenario: Search failure with previous results
- **GIVEN** visible package results and a subsequent failed request
- **WHEN** the Hub displays the error
- **THEN** previous results remain usable and a retry action is available without shifting the surrounding controls

### Requirement: Shared workbench registry configuration
The CLI and workbench SHALL use the existing data-only Ecky configuration owner for registry selection, and SHALL install through the same package store and integrity checks.

#### Scenario: Configure and use the public Hub
- **GIVEN** an existing Ecky profile
- **WHEN** the registry is configured and a package is installed from the Hub
- **THEN** configuration survives restart and installed exports enter the local SQLite catalog without rewriting historical model versions
