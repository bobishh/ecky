# Delta for component-library

## ADDED Requirements

### Requirement: Component extraction from existing parts

The system SHALL extract an existing part subtree into a closed
`define-component` via the compiler's binding resolution, producing
copy-inline source plus a header.

#### Scenario: Params become signature

- GIVEN a part whose body references model params `pole_od` and `clearance`
- WHEN `component_extract` runs against that part
- THEN the produced component signature contains both params with their
  metadata (defaults, min/max/step, labels) preserved.

#### Scenario: Scalar bindings become defaults

- GIVEN a part body referencing a scalar outer `let*` binding
- WHEN extraction runs
- THEN the binding becomes a signature entry whose default is its current
  evaluated value.

#### Scenario: Blocked extraction is explicit

- GIVEN a part body referencing a non-scalar outer binding (e.g. a shape)
- WHEN extraction runs
- THEN extraction fails with a blocker report naming the binding; no partial
  component is produced.

#### Scenario: Extracted component recompiles

- GIVEN any successful extraction
- WHEN the produced source is wrapped in a minimal model and instantiated
- THEN it compiles and plans without error.

### Requirement: Header contract

Each stored component SHALL carry a header with name, param manifest, tags,
provenance (threadId, messageId, sourceDigest), and referenced
named-constraint keys.

#### Scenario: Provenance recorded

- GIVEN extraction from a thread version
- WHEN the header is produced
- THEN it contains the thread id, message id, and source digest of the origin.

### Requirement: Library search returns headers only

`component_search` SHALL scan stored headers and return compact results
without component bodies; `component_get` SHALL return full copy-inline
source for one named component.

#### Scenario: Compact search results

- GIVEN a library with stored components
- WHEN an agent calls `component_search` with a query
- THEN results contain name, one-liner, param keys, and tags
- AND contain no body source.

#### Scenario: Copy-inline get

- GIVEN a stored component name
- WHEN an agent calls `component_get`
- THEN the response contains self-contained `.ecky` source pasteable into any
  model, including the component's verify clauses.
