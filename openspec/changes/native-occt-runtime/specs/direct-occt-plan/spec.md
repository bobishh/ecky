# Delta for direct-occt-plan

## ADDED Requirements

### Requirement: Core normalization before direct OCCT planning

The system SHALL normalize finite Core IR constructs before direct OCCT planning.

#### Scenario: Scalar branch resolves

- GIVEN a Core IR model with an `if` whose condition is scalar-evaluable from
  parameters
- WHEN direct OCCT normalization runs
- THEN only the selected branch remains
- AND direct OCCT planning receives no `if` node.

#### Scenario: Finite range and map expand

- GIVEN a Core IR model using finite `range` and `map` expressions to generate
  repeated geometry
- WHEN direct OCCT normalization runs
- THEN the dynamic expression expands into finite literal calls
- AND direct OCCT planning receives no `range` or `map` node.

#### Scenario: Apply expands

- GIVEN a Core IR model using `apply` over a finite list
- WHEN direct OCCT normalization runs
- THEN the apply expression expands into explicit operation calls
- AND direct OCCT planning receives no `apply` node.

### Requirement: Repeat family normalization

The system SHALL normalize repeat-family operations into direct OCCT plan-ready
operations.

#### Scenario: Repeat expands to instances

- GIVEN a Core IR model using `repeat`
- WHEN direct OCCT normalization runs
- THEN the result contains explicit transformed instances
- AND direct OCCT planning can consume the result.

#### Scenario: Repeat union expands to union

- GIVEN a Core IR model using `repeat-union`
- WHEN direct OCCT normalization runs
- THEN the result contains a `union` over expanded instances.

#### Scenario: Repeat compound expands to compound

- GIVEN a Core IR model using `repeat-compound`
- WHEN direct OCCT normalization runs
- THEN the result contains a `compound` over expanded instances.

#### Scenario: Repeat pick resolves selected item

- GIVEN a Core IR model using `repeat-pick`
- WHEN direct OCCT normalization runs
- THEN the result contains the selected generated item only.

### Requirement: Unsupported operation rejection before native compilation

The system SHALL reject unsupported direct OCCT operations before native
compilation.

#### Scenario: Xor reaches normalization

- GIVEN a Core IR model using `xor`
- WHEN direct OCCT normalization runs
- THEN normalization fails
- AND the error names `xor` as unsupported by direct OCCT.

#### Scenario: Unfilled hole reaches normalization

- GIVEN a Core IR model containing an unfilled typed `hole`
- WHEN direct OCCT normalization runs
- THEN normalization fails
- AND the error includes the requested hole type and goal.

#### Scenario: Mesh-only op reaches normalization

- GIVEN a Core IR model containing `wall-pattern`
- WHEN direct OCCT normalization runs
- THEN normalization fails
- AND the error states that the operation is mesh-only and cannot lower to BREP.

### Requirement: Native runner plan ABI

The system SHALL define a stable serialized plan contract for a precompiled
native OCCT runner.

#### Scenario: Runner consumes plan JSON

- GIVEN a normalized direct OCCT plan serialized as JSON
- WHEN the native runner executes it
- THEN the runner writes STEP, STL, and topology outputs
- AND the runner does not parse Ecky source.

#### Scenario: Runner rejects unknown schema

- GIVEN a plan JSON with unsupported `schemaVersion`
- WHEN the native runner executes it
- THEN execution fails before geometry creation
- AND the error names the unsupported schema version.

### Requirement: Keyword-free frame ops stay on the native runner

The system SHALL route keyword-free frame operations through the precompiled
native runner when the runner is available.

#### Scenario: Plane, location, path-frame, and place are keyword-free

- GIVEN a direct OCCT plan that uses only `plane`, `location`, `path-frame`,
  or `place` commands without keywords
- WHEN runner-first dispatch checks the plan
- THEN the plan is accepted by the native runner gate
- AND generated-source fallback is not required for those operations.

### Requirement: Narrow keyword arg ops stay on the native runner

The system SHALL route proven keyword-arg Direct OCCT plans through the
precompiled native runner when the runner is available.

#### Scenario: Profile holes use arg keywords

- GIVEN a direct OCCT plan whose `profile` command uses only `:outer` and
  `:holes` arg keywords with shape refs
- WHEN runner-first dispatch checks the plan
- THEN the plan is accepted by the native runner gate
- AND the runner emits topology targets with explicit `targetId` values.

#### Scenario: Clip-box uses numeric axis ranges

- GIVEN a direct OCCT plan whose `clip-box` command uses `:x`, `:y`, and `:z`
  numeric arg keywords
- WHEN runner-first dispatch checks the plan
- THEN the plan is accepted by the native runner gate
- AND generated-source fallback is not required for that operation.

### Requirement: Exact selector-id plans stay on the native runner

The system SHALL route proven exact target-id selector plans through the
precompiled native runner when the runner is available.

#### Scenario: Edge target ids drive fillet and chamfer

- GIVEN a direct OCCT plan whose `fillet` or `chamfer` command uses typed edge
  `targetIds`
- WHEN runner-first dispatch checks the plan
- THEN the plan is accepted by the native runner gate
- AND the native runner resolves exact or stable edge target ids before
  modifying the BREP.

#### Scenario: Face target ids drive shell

- GIVEN a direct OCCT plan whose `shell` command uses typed face `targetIds`
- WHEN runner-first dispatch checks the plan
- THEN the plan is accepted by the native runner gate
- AND the native runner resolves exact or stable face target ids before
  thick-solid generation.

### Requirement: Supported shell face clauses stay on the native runner

The system SHALL route proven shell face-clause selector plans through the
precompiled native runner when the runner is available.

#### Scenario: Face clauses drive shell

- GIVEN a direct OCCT plan whose `shell` command uses typed face clauses built
  from `boundary`, `planar`, `normal`, and `area`
- WHEN runner-first dispatch checks the plan
- THEN the plan is accepted by the native runner gate
- AND the native runner applies the same bbox/area clause semantics as the
  generated-source shell path before thick-solid generation.
