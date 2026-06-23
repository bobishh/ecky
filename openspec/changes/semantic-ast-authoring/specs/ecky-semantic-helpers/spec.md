# Delta for ecky-semantic-helpers

## ADDED Requirements

### Requirement: Intent-preserving cut helpers

The `.ecky` language SHALL provide semantic helper forms for recurring
subtractive patterns — at least through-wall slot, rear-panel circular
hole, front-opening rim cut, pocket/cavity cutter, centered port cut, and
side-button cut — that take named faces/sides and dimensioned arguments
instead of raw rotation angles, and whose AST preserves the helper form
(head, named arguments, span, stable id) rather than only the desugared
transform chain.

#### Scenario: Named side instead of rotation

- GIVEN a right-side through-wall slot authored via the helper
- WHEN the AST is inspected
- THEN the node states the helper kind and the named face `right`
- AND no consumer must decode a `(rotate 0 -90 0 ...)` chain to learn the
  cut direction.

#### Scenario: Helper intent reaches the projection

- GIVEN a model using a centered port cut helper
- WHEN the block-tree projection runs
- THEN the block presents the helper's intent (kind, face, dimensions)
- AND the desugared CSG subtree is reachable as derived detail, not as the
  primary representation.

### Requirement: Helpers desugar to existing CSG with backend parity

Each semantic helper SHALL desugar in the compiler to existing core
primitives and transforms only — introducing no new backend operations —
SHALL produce geometry equivalent to the hand-written CSG it replaces, and
SHALL pass the native-vs-build123d differential parity harness.

#### Scenario: Desugar equals hand-written CSG

- GIVEN a helper invocation and its documented hand-written CSG equivalent
- WHEN both models are rendered
- THEN bounding box and volume match within harness tolerance.

#### Scenario: All backends see only existing primitives

- GIVEN a model using every semantic helper
- WHEN it is lowered for native OCCT, build123d, and FreeCAD
- THEN each lowering contains only operations that existed before this
  change.

### Requirement: Helpers eliminate coincident-face cut authoring

Through-cutting helpers SHALL extend the cutting solid beyond the walls
they pierce by a defined over-cut margin, so that a cut authored via a
helper cannot terminate exactly on a target surface.

#### Scenario: Through-slot over-cuts both walls

- GIVEN a through-wall slot helper applied to a wall of thickness `t`
- WHEN the helper desugars
- THEN the cutting solid's extent exceeds `t` on both sides of the wall
- AND the resulting boolean produces a manifold through-opening.
