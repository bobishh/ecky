## ADDED Requirements

### Requirement: Preview edited parameters in the active model

The workbench SHALL show approximate display geometry for draft parameter edits to a saved Ecky model that can run on the native Direct OCCT path. Both the parameter panel and viewport overlay controls SHALL edit one shared draft bound to the selected version, dispatch updates with leading-edge and latest-wins scheduling during continuous input, and keep the saved model visible.

#### Scenario: Edit a dimension of a placed part

- **GIVEN** a saved model with a rendered part and an editable parameter
- **WHEN** the user changes the value in the parameter panel or drags a slider
- **THEN** the scene shows a translucent coarse mesh of the affected geometry at its model position
- **AND** the saved model, current export artifacts, and version history remain available
- **AND** streaming through several values dispatches leading-edge updates, coalesces queued edits to the latest value, and retains the previous ghost while updating

#### Scenario: Edit a shared parameter from the viewport overlay with Parameters closed

- **GIVEN** a saved multi-part model (`left` and `right` bulbs plus a `stand`) with a shared `radius` parameter
- **WHEN** the user closes the Parameters window and edits `radius` from the viewport overlay
- **THEN** the viewport stays in selection mode and updates the shared version draft without marking the working copy dirty
- **AND** the preview shows per-part ghost meshes for `left` and `right` while `stand` remains fixed
- **AND** if a sibling part is hidden in the assembly, its ghost mesh remains hidden

### Requirement: Keep draft output separate from saved results

The preview SHALL build display meshes using coarse tessellation with per-part identity. It SHALL exclude production export, analysis, and history outputs. The preview mesh SHALL be excluded from canonical viewport screenshots.

#### Scenario: Cancel a draft from panel or viewport

- **GIVEN** a parameter edit with a pending or visible preview
- **WHEN** the user selects Cancel in the parameter panel or viewport overlay
- **THEN** the saved values are restored and the draft mesh disappears
- **AND** in-flight preview work is cancelled and a late response cannot restore the draft
- **AND** no version or export changes

#### Scenario: Apply a draft

- **GIVEN** draft parameter values in the shared draft
- **WHEN** the user selects Apply in the parameter panel or viewport overlay
- **THEN** the display-tier version process commits the chosen values with per-part meshes and bounds
- **AND** the ghost is cleared
- **AND** production STEP and fine STL artifacts materialize on demand when exporting or analyzing

#### Scenario: A draft cannot render

- **GIVEN** a draft whose preview cannot be built
- **WHEN** the preview returns an error
- **THEN** the status displays the error and keeps the saved model visible
- **AND** the user can adjust the draft or cancel it
