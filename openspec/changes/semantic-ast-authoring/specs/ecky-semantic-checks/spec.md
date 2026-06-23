# Delta for ecky-semantic-checks

## ADDED Requirements

### Requirement: Node-attached semantic checks

The `.ecky` language SHALL support verify-time checks that name semantic
nodes (bindings or groups) and assert relationships between them — at least:
cutter intersects a target solid, cutter reaches a named cavity, wall
thickness between two shapes stays above a minimum, a hole contains an
expected point, and a cut is centered on a named face along an axis. Checks
SHALL attach to the named node, not only to the whole exported artifact.

#### Scenario: Cutter-intersects check

- GIVEN a check asserting `pocket-cutter` intersects `body`
- WHEN verification runs on a model where the cutter misses the body
- THEN the check fails with the measured intersection result
- AND the failure names `pocket-cutter` as its anchor node.

#### Scenario: Minimum wall check

- GIVEN a check asserting the wall between the pocket and the outer face is
  at least the declared minimum
- WHEN a parameter change makes the wall thinner than the minimum
- THEN verification fails with measured and required thickness.

#### Scenario: Centered cut check

- GIVEN a check asserting a port cut is centered on the bottom face along x
- WHEN the cut is authored off-center
- THEN the check fails with the measured offset.

### Requirement: Check results carry anchor node ids

Semantic check results SHALL include the stable node id of the binding or
group they concern, in the format accepted by the block-view node-attached
verification contract, so failures render at the relevant block without UI
changes.

#### Scenario: Result anchors to the checked node

- GIVEN a failing cutter-reaches check on `usb-cut`
- WHEN results are emitted
- THEN the result carries the node id of the `usb-cut` binding
- AND the block view can attach the failure to that block.

### Requirement: Semantic checks join the existing verify discipline

Semantic checks SHALL run in the existing verify pipeline with the same
consequences as authored `verify` clauses: red results feed deterministic
feedback into the generation retry loop, block silent commit of the
version, and are reported honestly when the retry cap is reached.

#### Scenario: Red semantic check blocks commit

- GIVEN a generated model whose semantic check fails
- WHEN the generation loop evaluates the render
- THEN the version is not committed as a successful result
- AND the check failure with its anchor and measurements enters the retry
  feedback.

#### Scenario: Every check can fail

- GIVEN the test suite for a semantic check kind
- WHEN the suite runs
- THEN it includes a known-bad fixture that the check fails
- AND a known-good fixture that the check passes.
