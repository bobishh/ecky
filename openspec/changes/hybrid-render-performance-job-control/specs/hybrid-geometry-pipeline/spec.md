# Delta for hybrid-geometry-pipeline

## ADDED Requirements

### Requirement: Representation-aware hybrid execution

The system SHALL preserve exact BRep and indexed manifold mesh as distinct
representations and SHALL choose a Boolean kernel from the operation and export
contract instead of converting every mesh to faceted BRep.

#### Scenario: Mesh island exported as STL or 3MF

- GIVEN a validated indexed manifold mesh participates in a hybrid Boolean
- AND requested output is STL or 3MF
- WHEN the hybrid plan executes
- THEN local exact operands are tessellated for the mesh island
- AND the Boolean runs in the admitted mesh kernel
- AND the result remains an indexed manifold mesh

#### Scenario: Analytic STEP remains exact

- GIVEN a part contains only exact BRep operations
- WHEN STEP export is requested
- THEN the part remains on the OCCT path
- AND no mesh Boolean or faceted conversion occurs

#### Scenario: Faceted STEP exceeds budget

- GIVEN an imported mesh requires faceted STEP
- AND projected faceted-BRep faces exceed configured budget
- WHEN plan validation runs
- THEN execution is rejected with projected count and budget
- AND no hidden kernel fallback occurs

### Requirement: N-ary Boolean execution

The system SHALL batch union and head-minus-tail difference operands while
preserving authored semantics. N-way intersection MUST retain
intersection-of-all semantics.

#### Scenario: Multi-operand union

- GIVEN a union with three or more operands
- WHEN the OCCT or mesh plan executes
- THEN operands are submitted to one n-ary builder

#### Scenario: Multi-tool difference

- GIVEN a difference with one target and multiple tools
- WHEN the plan executes
- THEN the target is the sole argument
- AND remaining operands are one ordered tool group

#### Scenario: Multi-operand intersection

- GIVEN an intersection with three or more operands
- WHEN the plan executes
- THEN result equals the region common to every operand
- AND operands are not lowered as `head ∩ union(tail)`

### Requirement: Native runner admission is exact and diagnosable

The system SHALL validate Direct OCCT runner operation, argument, keyword, and
selector forms through one admission contract before starting the runner.

#### Scenario: Partial clip-box is rejected before execution

- GIVEN a planned `clip-box` provides `:z` but omits required `:x` and `:y`
- WHEN Direct OCCT admission runs
- THEN no runner process starts
- AND the error identifies the part, output slot, `clip-box` operation, and
  missing `:x` and `:y` keywords

#### Scenario: Unsupported command form remains actionable without fallback

- GIVEN an operation exists in the runner but its planned keyword or selector
  form is outside the runner ABI
- WHEN Direct OCCT admission runs
- THEN the error identifies the exact part, output slot, operation, and form
- AND removal of generated-C++ fallback does not replace that cause with a
  generic unsupported-plan error

### Requirement: Deterministic hybrid reuse

The system SHALL reuse successful immutable hybrid artifacts by content and
coalesce identical concurrent work without caching failures.

#### Scenario: Warm identical render

- GIVEN a verified artifact exists for identical inputs and runtime identity
- WHEN the model renders again
- THEN no geometry kernel process starts
- AND the verified artifact bundle is returned

#### Scenario: Concurrent identical render

- GIVEN two subscribers request the same uncached artifact
- WHEN requests overlap
- THEN one kernel job executes
- AND both subscribers receive the same result or raw failure

### Requirement: Hybrid progress and cancellation

The system SHALL expose typed stage progress and subscriber-aware cancellation
for long-running kernel jobs.

#### Scenario: Long Boolean reports progress

- GIVEN a hybrid Boolean is running
- WHEN kernel stages advance
- THEN subscribers receive typed import, validation, Boolean, verification, and
  export progress
- AND interactive kernel output is not copied into app logs

#### Scenario: Last subscriber cancels

- GIVEN a shared kernel job has one remaining subscriber
- WHEN that subscriber cancels
- THEN cooperative cancellation is requested
- AND an uncooperative child is terminated
- AND no partial artifact enters cache
