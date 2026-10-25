## ADDED Requirements

### Requirement: Component collections and exact dependencies
The package system SHALL support a release exporting one component or a collection, with a bounded exact dependency graph resolved before compilation.

#### Scenario: Install a collection
- **GIVEN** a package containing several exports and exact dependencies
- **WHEN** the user installs it
- **THEN** all required verified releases become available to discovery and source import

#### Scenario: Invalid graph
- **GIVEN** cyclic dependencies or a conflicting immutable coordinate
- **WHEN** installation resolves the graph
- **THEN** it fails with the offending dependency path before publishing incomplete discovery state

### Requirement: One canonical lock
The system SHALL extend the existing canonical model-version dependency lock compatibly and SHALL distinguish registry origin, archive SHA-256 and canonical payload digest.

#### Scenario: Historical lock after a newer installation
- **GIVEN** an old committed model lock and a newer installed package
- **WHEN** the model is reopened, rendered or exported
- **THEN** it uses its original locked payload and does not rewrite the lock

### Requirement: Frozen and offline installation
The CLI SHALL support frozen installation from an unchanged lock and offline installation from verified cached content.

#### Scenario: Frozen declaration mismatch
- **GIVEN** dependency declarations differ from the lock
- **WHEN** frozen install runs
- **THEN** it fails without selecting replacement versions or silently rewriting the lock

#### Scenario: Offline cached graph
- **GIVEN** every locked payload is cached and verifies
- **WHEN** offline install runs without network access
- **THEN** the same locked package graph is available

### Requirement: Cairn command workflow
The CLI SHALL provide shared `cairn` and `ecky cairn` workflows for search, info, init, pack, publish, add, install, explicit update and catalog recovery, with machine-readable output.

#### Scenario: Package round trip
- **GIVEN** a valid package directory
- **WHEN** it is packed, published, found, installed and imported
- **THEN** the exact exported component compiles with its expected contract and provenance

### Requirement: Verified content store publication
Installation SHALL reuse the existing global content-addressed store and SHALL reject corrupt archives, unsafe paths, excessive expansion and digest mismatches before making a release discoverable.

#### Scenario: Corrupt download
- **GIVEN** downloaded bytes differ from the pinned archive or payload digest
- **WHEN** install validates them
- **THEN** the prior store/discovery state remains usable and the mismatched release is rejected

### Requirement: Explicit source reuse modes
The system SHALL distinguish pinned live package reuse from copy-inline adaptation and SHALL preserve the existing native STEP import path.

#### Scenario: Adapt selected logic
- **GIVEN** an inspected source component
- **WHEN** a design explicitly vendors and adapts its logic
- **THEN** provenance remains recorded and no hidden live dependency is introduced

### Requirement: Installable CLI distribution
The project SHALL publish versioned macOS and Linux CLI archives containing the `ecky` and `cairn` clients, checksums and an installer that verifies the archive before replacing an existing installation.

#### Scenario: One-command client installation
- **GIVEN** a supported macOS or Linux machine without a source checkout
- **WHEN** the user runs the documented installer
- **THEN** the matching released CLI archive is downloaded and integrity-checked and both commands become available without building the desktop application

#### Scenario: Interrupted or invalid download
- **GIVEN** an existing CLI installation
- **WHEN** download or checksum verification fails
- **THEN** the installer fails with an actionable error and preserves the existing executables
