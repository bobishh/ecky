# Delta for ecky-stdlib

## ADDED Requirements

### Requirement: A versioned standard library of components ships with Ecky

The system SHALL provide a standard library of parametric parts authored as
`.ecky` `define-component` definitions, each self-contained (copy-inlineable),
versioned, and carrying its own `verify` clauses.

#### Scenario: Every shipped stdlib component compiles and verifies

- GIVEN a component in the shipped stdlib
- WHEN it is instantiated with default parameters and rendered on the default backend
- THEN it compiles
- AND its embedded `verify` clauses pass.

#### Scenario: stdlib components are discoverable by search

- GIVEN the stdlib manifest
- WHEN a client calls component search
- THEN stdlib components appear with name, summary, param keys, tags, and version
- AND their full source is not returned by search.

### Requirement: Components import copy-inline into a model

The system SHALL let an author insert an stdlib or user-library component into
the active model as instantiated, self-contained `define-component` source, with
no implicit registry reference. Import SHALL be invocable both by the agent (an
MCP tool) and by a human (a workbench panel), with both routed through one
shared import path so the result is identical regardless of entry point.

#### Scenario: Import inlines self-contained source

- GIVEN a chosen stdlib component
- WHEN it is imported into the active model
- THEN the model gains the component's full `define-component` source plus an instantiation
- AND the model compiles without any external library reference.

#### Scenario: Tool and panel imports are identical

- GIVEN the same component is imported once via the MCP tool and once via the
  workbench panel
- WHEN each import completes
- THEN both produce the same inlined source and instantiation
- AND neither path requires the other to be present.

#### Scenario: Import records the component version

- GIVEN an stdlib component pinned at a version
- WHEN it is imported
- THEN the inlined source records the version it was taken from
- AND a later stdlib update does not silently change the model.
