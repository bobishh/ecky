# Delta for macro-ast-map

## ADDED Requirements

### Requirement: Part nodes collapse past a density threshold

The map layout SHALL render a part node whose parameter count exceeds a
density threshold as a collapsed node — header, syntax badge, and a
parameter-count chip — with a constant height independent of parameter count,
and SHALL omit that part's parameter module nodes from the scene until the
part is explicitly expanded. Parts at or under the threshold render expanded
as today. Expansion state is an explicit per-node toggle remembered for the
session.

#### Scenario: Dense part renders collapsed

- GIVEN a model whose part binds more parameters than the density threshold
- WHEN the New Params map renders
- THEN the part node shows its label and a parameter-count chip
- AND no inline parameter controls for that part are present in the scene
- AND the part node height does not grow with the parameter count.

#### Scenario: Expanding restores the full param grid

- GIVEN a collapsed dense part node
- WHEN the author toggles it expanded
- THEN the parameter modules render in the balanced column grid as before
- AND inline editing of those parameters behaves unchanged
- AND collapsing it again removes the modules and restores the compact height.

#### Scenario: Focus flows auto-expand the owning part

- GIVEN a collapsed dense part that owns parameter `wall_thickness`
- WHEN a focus flow targets that parameter (highlightParam, error diagnostic
  focus, or param activation)
- THEN the owning part expands first
- AND the parameter control receives focus as it does for expanded parts.

### Requirement: Node source editing is scoped to the node slice

The source pane SHALL edit only the selected node's byte slice: the editor
document is the slice text, and APPLY splices the edited slice back into the
base document snapshot taken when the pane opened, submitting the spliced
whole through the existing apply flow. The pane SHALL NOT expose the rest of
the macro source for editing.

#### Scenario: Pane shows only the node's source

- GIVEN a model with parts `part_a` and `part_b`
- WHEN the author double-clicks the `part_a` node
- THEN the source pane contains the `part_a` expression
- AND the pane does not contain the `part_b` expression.

#### Scenario: Applying a slice edit rewrites only that range

- GIVEN an open pane scoped to `part_a`
- WHEN the author edits the slice and clicks APPLY
- THEN the submitted macro equals the base document with only the `part_a`
  byte range replaced by the edited slice
- AND the preview rerenders through the existing apply flow.

#### Scenario: ADD PART edits only the inserted template

- GIVEN the author clicks the ADD PART ghost slot
- WHEN the source pane opens
- THEN the pane contains only the inserted part template
- AND applying it splices the edited template into the insertion point.

### Requirement: Dirty pane refuses silent scope switches

While the source pane holds unsaved edits, the system SHALL refuse to replace
the pane content or its byte offsets when a different node is selected for
source editing; the pane SHALL keep the current draft and surface an inline
message that the draft must be applied or closed first. With a clean pane,
switching nodes SHALL replace the slice, label, and offsets.

#### Scenario: Dirty draft blocks node switch

- GIVEN an open pane for `part_a` whose text has been modified
- WHEN the author double-clicks the `part_b` node
- THEN the pane still shows the modified `part_a` draft
- AND an inline message states the draft must be applied or closed first.

#### Scenario: Clean pane follows node selection

- GIVEN an open pane for `part_a` with no modifications
- WHEN the author double-clicks the `part_b` node
- THEN the pane shows the `part_b` slice and label.
