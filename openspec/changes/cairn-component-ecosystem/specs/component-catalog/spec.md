## ADDED Requirements

### Requirement: Persisted metadata queries
The system SHALL serve normal catalog browsing and search from persisted SQLite metadata without decoding model history, parsing source, reading per-component header files, rendering geometry or generating document embeddings during the request. Explicit semantic queries MAY obtain a bounded query embedding from the configured local provider; ordinary browsing SHALL NOT invoke it.

#### Scenario: History writer is occupied
- **GIVEN** previously indexed components and a held history writer lock
- **WHEN** Library or MCP searches the catalog
- **THEN** indexed results are returned independently of that lock

#### Scenario: Source storage is temporarily unavailable
- **GIVEN** a published catalog snapshot
- **WHEN** source/header files cannot be read
- **THEN** compact metadata remains searchable and exact source retrieval reports its own actionable failure

### Requirement: Scope-correct reusable source
The system SHALL resolve supported model-local definitions and lexical loop bindings consistently with the compiler and preserve the transitive dependency closure of a captured component.

#### Scenario: Model-local component call
- **GIVEN** a part calling a component declared directly inside its model and using a transitive helper
- **WHEN** successful source is captured
- **THEN** the part and reusable definitions are indexed and their captured source compiles in a fresh model

#### Scenario: Nested loop indices
- **GIVEN** nested `for-union` or `for-compound` bindings
- **WHEN** free references are collected
- **THEN** loop indices are local to their bodies while count expressions retain outer-scope dependencies

### Requirement: Asynchronous recovery
The system SHALL recover missing or obsolete catalog projections outside search requests, report progress and retain an available published snapshot during recovery.

#### Scenario: Old error cursor
- **GIVEN** an obsolete checkpoint containing false extraction diagnostics
- **WHEN** catalog recovery runs under the new capture schema
- **THEN** source is reprocessed and successful current metadata replaces the obsolete diagnostics

#### Scenario: Recovery failure
- **GIVEN** existing indexed components and an unreadable source version
- **WHEN** recovery reaches that source
- **THEN** existing useful metadata remains available and the workbench exposes error details plus a rebuild action

### Requirement: Latest publication and retained history
The system SHALL publish a capture only if its source is still the latest eligible successful version, remove obsolete discovery rows and preserve immutable revisions referenced by historical models.

#### Scenario: Late old completion
- **GIVEN** a newer successful version was published during extraction
- **WHEN** an older capture attempts publication
- **THEN** it cannot regress current catalog references

### Requirement: Explicit index ownership
The system SHALL treat catalog entries and checkpoints as derived projections with one SQLite owner and SHALL migrate legacy headers without using old cursor files as current successful checkpoints.

#### Scenario: Repeated query after migration
- **GIVEN** completed legacy header migration
- **WHEN** the same catalog is queried repeatedly
- **THEN** normal reads do not rescan legacy directories
