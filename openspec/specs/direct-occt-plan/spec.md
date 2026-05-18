# direct-occt-plan Specification

## Purpose

Define the Rust-owned plan contract between normalized Ecky Core IR and native
OpenCascade execution.

## Requirements

### Requirement: Core IR planning

The system SHALL translate supported Core IR operations into a direct OCCT plan.

#### Scenario: Supported model plans

- GIVEN a verified Core IR model using supported BREP operations
- WHEN direct OCCT planning runs
- THEN each part produces ordered `OcctCommand` entries
- AND the plan identifies each part root slot.

#### Scenario: Unsupported operation rejects

- GIVEN a Core IR model containing an unsupported direct OCCT operation
- WHEN direct OCCT planning runs
- THEN planning fails before native compilation
- AND the error names the unsupported operation.

### Requirement: Dynamic Core IR rejection

The system SHALL not send dynamic Core IR nodes to native OCCT planning.

#### Scenario: Dynamic node reaches planner

- GIVEN a Core IR model still containing `if`, `range`, `map`, or `apply`
- WHEN direct OCCT planning runs
- THEN planning rejects the model
- AND the error states that dynamic expressions must be evaluated before
  planning.

### Requirement: Plan execution artifacts

The system SHALL execute direct OCCT plans into STEP, STL, and topology outputs.

#### Scenario: Plan export succeeds

- GIVEN a valid direct OCCT plan
- WHEN native execution runs
- THEN `model.step` is written
- AND `preview.stl` is written
- AND `topology.json` is written.
