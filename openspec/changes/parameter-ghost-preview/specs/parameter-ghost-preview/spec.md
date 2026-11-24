## ADDED Requirements

### Requirement: Preview edited parameters in the active model

The workbench SHALL show approximate display geometry for draft parameter edits to a saved Ecky model that can run on the native Direct OCCT path. It SHALL request the preview after a pause in input and keep the saved model visible.

#### Scenario: Edit a dimension of a placed part

- **GIVEN** a saved model with a rendered part and an editable parameter
- **WHEN** the user changes the value and pauses
- **THEN** the scene shows a translucent mesh of the affected geometry at its model position
- **AND** the saved model, current export artifacts, and version history remain available
- **AND** moving through several values displays the result for the latest request and retains the previous ghost while updating

#### Scenario: Edit a shared parameter

- **GIVEN** a saved model with a parameter used by multiple parts
- **WHEN** the user changes that parameter
- **THEN** the preview includes the affected parts with their resolved placements and dependencies

### Requirement: Keep draft output separate from saved results

The preview SHALL build display meshes using coarse tessellation. It SHALL exclude production export, analysis and history outputs. The preview mesh SHALL be excluded from canonical viewport screenshots.

#### Scenario: Cancel a draft

- **GIVEN** a parameter edit with a pending or visible preview
- **WHEN** the user selects Cancel
- **THEN** the saved values are restored and the draft mesh disappears
- **AND** a late response cannot restore the draft
- **AND** no version or export changes

#### Scenario: Apply a draft

- **GIVEN** draft parameter values in the workbench
- **WHEN** the user selects Apply
- **THEN** the existing full render and version process saves the chosen values
- **AND** the ghost is cleared
- **AND** the resulting export artifacts come from the production render

#### Scenario: A draft cannot render

- **GIVEN** a draft whose preview cannot be built
- **WHEN** the preview returns an error
- **THEN** the parameter panel shows the error and keeps the saved model visible
- **AND** the user can adjust the draft or cancel it
