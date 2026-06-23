# Delta for ast-block-view

## ADDED Requirements

### Requirement: Semantic block tree projection

The system SHALL project a `.ecky` model into a semantic block tree — model,
params, parts, `let*` bindings, construction groups, final booleans, verify
clauses — where every node carries a stable id and the exact byte span of its
source expression, and the tree is regenerated from source on every parse.

#### Scenario: Part let* bindings become addressable blocks

- GIVEN a part whose `let*` defines dozens of bindings
- WHEN the block tree projection runs
- THEN each binding is a node with a stable id and byte span
- AND the node ids follow the shared AST identity scheme used by the map view.

#### Scenario: Long let* is grouped, not flat

- GIVEN a part with derived dimensions, solids, cutters, and a final difference
- WHEN the block tree is rendered
- THEN the bindings appear as collapsible construction groups
- AND section comments between bindings title their groups
- AND the final boolean chain is its own group.

#### Scenario: Collapsing never hides source access

- GIVEN a collapsed block
- WHEN the author expands or inspects it
- THEN the original Lisp form is reachable unchanged
- AND no block-view state exists that cannot be regenerated from source.

### Requirement: Visible inferred shape roles

The system SHALL assign each block a role — parameter, derived value, profile,
solid, cutter, transform, boolean, or verification — inferred from binding
names, expression heads, and boolean usage, and SHALL display both the role
and the fact that it was inferred.

#### Scenario: Name-suffix inference

- GIVEN bindings named `corner-bumper-solid` and `camera-opening-cutter`
- WHEN roles are inferred
- THEN the first is shown as a solid and the second as a cutter
- AND each role is marked as inferred from the name.

#### Scenario: Boolean usage marks cutters

- GIVEN a binding referenced only as a non-first operand of `difference`
- WHEN roles are inferred
- THEN the binding is shown as a cutter regardless of its name
- AND a conflict with a name-based role is surfaced, not silently resolved.

#### Scenario: Profile constructors read as profiles

- GIVEN a binding whose value is a `rounded-rect` expression
- WHEN roles are inferred
- THEN the binding is shown as a profile.

### Requirement: Derived values show formula, value, and provenance

For scalar bindings the system SHALL display the source formula together with
its evaluated value under current parameters, and SHALL let the user navigate
from each referenced identifier to the block that defines it.

#### Scenario: Formula and evaluated value together

- GIVEN `(outer-case-width (+ phone-pocket-width (* 2.0 side-wall-thickness)))`
- WHEN the block is displayed
- THEN it reads as the name, the evaluated value, and the formula
- AND changing a referenced parameter updates the evaluated value.

#### Scenario: Reference navigation

- GIVEN a derived-value formula referencing `side-wall-thickness`
- WHEN the user clicks the reference
- THEN the block defining `side-wall-thickness` is selected and revealed.

#### Scenario: Runtime-only values degrade honestly

- GIVEN a binding that cannot be evaluated statically
- WHEN the block is displayed
- THEN the formula is shown with its value marked as runtime
- AND no fabricated number is displayed.

### Requirement: Compact geometry summaries

For shape expressions the system SHALL provide a collapsed one-line summary of
the transform chain and base operation, expandable to translation vector,
rotation angles, extrusion depth and direction, and profile dimensions.

#### Scenario: Button cutter summarizes

- GIVEN `(translate x y z (rotate 0 -90 0 (extrude (rounded-rect h l r) d)))`
- WHEN the block is collapsed
- THEN it summarizes as a translated rotated extrusion
- AND expanding shows the translation vector, rotation angles, extrusion depth,
  and profile dimensions.

#### Scenario: Final boolean shows base and cutters

- GIVEN a part result `(difference base-solid cutter-a cutter-b)`
- WHEN the final boolean block is displayed
- THEN the base solid and each subtractive operand are listed explicitly
- AND subtractive operands carry cutter badges.

### Requirement: Intermediate shape debug previews

The system SHALL let the user preview, from any binding block: the binding's
shape alone, the accumulated model up to that binding, or all cutter-role
shapes together — rendered as preview-only debug geometry that never enters
export output.

#### Scenario: Preview a single cutter

- GIVEN the user selects a cutter binding
- WHEN they request a shape-only preview
- THEN the viewport renders only that cutter's geometry.

#### Scenario: Preview accumulated construction state

- GIVEN the user selects a mid-`let*` binding
- WHEN they request an accumulated preview
- THEN the model is rendered as if the `let*` ended at that binding.

#### Scenario: Debug geometry stays out of exports

- GIVEN any debug preview has been rendered
- WHEN the model is exported to STL or STEP
- THEN the export contains no debug preview primitives.

### Requirement: Node-attached verification results

The system SHALL attach verification results to the block they concern — the
binding or part their selectors name — and SHALL accept future checks that
emit results carrying an anchor node id without UI changes.

#### Scenario: Failure renders at the relevant block

- GIVEN a verify clause about a specific part fails
- WHEN results are displayed
- THEN the failure appears at that part's block with the raw message
- AND not only in a global log.

#### Scenario: Unanchorable results fall back to the part

- GIVEN a verify clause whose selectors resolve to no single binding
- WHEN results are displayed
- THEN the result attaches to the enclosing part block.

### Requirement: Bidirectional source and block synchronization

The system SHALL keep the source text and the block view synchronized in both
directions using byte spans from the same parse.

#### Scenario: Block selection highlights source

- GIVEN the user selects a block
- WHEN the selection is applied
- THEN the source pane highlights exactly that node's byte span.

#### Scenario: Source cursor selects block

- GIVEN the user places the cursor inside an expression in the source pane
- WHEN the selection is applied
- THEN the innermost block containing that offset is selected
- AND its collapsed ancestors are expanded to reveal it.

### Requirement: Parameter editing is the only block-view mutation

The system SHALL allow editing parameter values from the block view through
the existing structured AST-patch flow, and SHALL NOT mutate source through
any other block-view interaction.

#### Scenario: Parameter edit propagates

- GIVEN the user edits a parameter value in the block view
- WHEN the patch is accepted
- THEN derived values and downstream previews update from the new source.

#### Scenario: Rejected edit surfaces raw error at the node

- GIVEN the backend rejects a parameter value
- WHEN the result is displayed
- THEN the source stays unchanged
- AND the raw backend error body is shown at the parameter block.
