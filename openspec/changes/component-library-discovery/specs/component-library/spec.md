## ADDED Requirements

### Requirement: Workbench discovers saved and shipped components

The Components view SHALL show shipped standard-library and locally extracted
component summaries as well as installed package headers. Discovery SHALL reuse
the existing component store and SHALL NOT require source body reads or package
installation for extracted components.

#### Scenario: Local components without packages

- **GIVEN** a saved component header and no installed packages
- **WHEN** the user opens Library Components
- **THEN** its name, description, parameters, and local origin are visible
- **AND** shipped component summaries are also visible

#### Scenario: Search and refresh

- **WHEN** the user searches a component name, description, parameter, tag, or port
- **THEN** matching summaries remain visible and a no-match result is explicit
- **WHEN** the user refreshes after saving another component
- **THEN** discovery is reloaded from the existing store

#### Scenario: Storage failure

- **WHEN** the component store cannot be read
- **THEN** the Components view shows actionable backend error detail and retry
- **AND** it does not present the failure as an empty library

### Requirement: Successful versions capture authored reusable components

The Rust-owned successful version persistence path SHALL discover authored
top-level `define-component` forms after a successful render. Capture SHALL store
complete local dependency closure as immutable source revisions. It SHALL NOT
promote ordinary parts, mutate model source, append model versions, or delete
saved components when a source definition disappears.

#### Scenario: Repeated identical definition

- **GIVEN** a successful rendered source with a `define-component`
- **WHEN** the same definition renders again with whitespace, comments, instance
  parameters, or placement changed
- **THEN** the stable component identity and latest saved revision remain the same
- **AND** no revision is written for the changed instance or formatting

#### Scenario: Definition or dependency changes

- **GIVEN** a saved component with local top-level helper dependencies
- **WHEN** its definition or a transitive helper/default/port dependency changes
- **THEN** a new immutable revision becomes latest under the same component ID
- **AND** the previous revision remains readable by its exact digest
- **AND** an identically named component in another thread has a different ID

#### Scenario: Failed render and ordinary part

- **WHEN** a render fails
- **THEN** no component capture occurs from that failed version
- **WHEN** a successful source contains only an ordinary `part`
- **THEN** no component is created

#### Scenario: Existing successful source and unresolved dependency

- **WHEN** the Components view loads or MCP `component_search` runs
- **THEN** the latest successful rendered source for each thread is backfilled
  without changing version history
- **WHEN** an authored component depends on unresolved source
- **THEN** capture reports an indexing diagnostic and does not advertise incomplete
  component source as reusable

### Requirement: Local component revisions have portable identity

Every automatically captured revision SHALL include a stable thread-scoped
`componentId`, schema version, semantic `revisionDigest`, explicit dependencies,
and source provenance. The library SHALL provide a shared reader by component ID
and optional exact revision digest. Latest revision identity SHALL follow an
explicit atomically updated pointer, never filename ordering or guessed hashes.

#### Scenario: Revision-pinned read

- **GIVEN** a component has multiple retained revisions
- **WHEN** a caller reads by `componentId` and an older exact `revisionDigest`
- **THEN** the matching source and provenance are returned even after latest moves
- **AND** altered or mismatched identity/digest data is rejected
