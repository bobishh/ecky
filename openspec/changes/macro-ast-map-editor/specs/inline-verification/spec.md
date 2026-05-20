# Delta for inline-verification

## ADDED Requirements

### Requirement: Verification renders as first-class map layer

The system SHALL render authored verification intent and runtime verification
results directly on the AST map.

#### Scenario: Verify clause appears on map

- GIVEN `.ecky` source contains an authored top-level verify clause
- WHEN the AST map editor renders
- THEN the verify clause appears as a verification node or overlay
- AND it is anchored to related source-backed AST node ids when references are
  resolvable.

#### Scenario: Verification states render distinctly

- GIVEN runtime verification has pending, pass, fail, and error results
- WHEN the verification layer renders
- THEN each state is visually distinct
- AND failure or error state exposes raw backend/provider error body at the
  verification node.

### Requirement: Authors can create named verification constraints from map

The system SHALL let authors create named physical fit verification from
selected map or geometry entities.

#### Scenario: Distance verification from two selections

- GIVEN the author selects two faces, parts, ports, or fit-related AST nodes
- WHEN the author creates a distance verification
- THEN the backend creates a named constraint or named binding
- AND a source-backed verify clause is added or updated
- AND the new verification node appears on the map.

#### Scenario: Invalid verification selection reports at selection

- GIVEN the author selects entities that cannot define a valid verification
  relation
- WHEN the author attempts to create verification
- THEN source is not mutated
- AND the raw backend diagnostic appears at the selected nodes or pending verify
  node.

### Requirement: Verification diagnostics never enter export geometry

The system SHALL keep verification overlays and debug diagnostics out of
production export geometry.

#### Scenario: Debug overlay excluded from STL STEP

- GIVEN a model has verification overlays visible in preview
- WHEN production STL or STEP export runs
- THEN export geometry contains authored model geometry only
- AND no debug overlay primitives are emitted.

#### Scenario: Verification result links remain source-backed

- GIVEN verification runtime reports a failed named constraint
- WHEN the AST map displays the failure
- THEN the failure links to the source-backed verify node id
- AND selecting the failure can reveal the related source context.
