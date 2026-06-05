# Delta for code-inspector

## ADDED Requirements

### Requirement: Inspector shows source only, no inline diff

The code inspector SHALL present the current model source for editing without an
inline diff panel.

#### Scenario: Applying a code draft shows no diff panel

- GIVEN a code draft has just been applied in the inspector
- WHEN the inspector renders
- THEN no `.code-diff` / `code-diff-panel` element is present
- AND the editor shows the current source.

### Requirement: Single fork affordance

The inspector footer SHALL NOT provide its own fork action; forking is reached
through the viewport fork control only.

#### Scenario: Inspector footer has no fork button

- GIVEN the code inspector is open in version mode
- WHEN the footer renders
- THEN there is no `FORK TO NEW THREAD` button
- AND `APPLY` and `COMMIT VERSION` remain available.

### Requirement: Commit fields are labeled and separated from actions

The Title and Version-name commit fields SHALL be rendered as clearly labeled
text inputs, visually distinct from the action buttons.

#### Scenario: Commit fields read as inputs

- GIVEN the code inspector is open in version mode
- WHEN the footer renders
- THEN the Title input has a visible label
- AND the Version-name input has a visible label
- AND both are grouped apart from the COPY/APPLY/COMMIT buttons.
