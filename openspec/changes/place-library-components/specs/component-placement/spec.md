## ADDED Requirements

### Requirement: Preview a library component in the current model

The desktop workbench SHALL let users stage a built-in, pinned local, or installed package source component in the active Ecky model. It SHALL render the selected component by itself and display it as a translucent object in model coordinates without saving a model version.

#### Scenario: Choose a component

- **GIVEN** a current Ecky source version and a source component in the library
- **WHEN** the user chooses Place in scene
- **THEN** a small non-modal window identifies the component and shows its X, Y, and Z coordinates in millimetres
- **AND** the scene shows the staged component and its translation arrows when the preview loads
- **AND** the saved source, history, and exportable model stay unchanged

#### Scenario: Move a staged component

- **GIVEN** the staged component is visible
- **WHEN** the user picks an initial point on the XY plane, drags a translation arrow, or enters a coordinate
- **THEN** the staged component and coordinate fields reflect the same position
- **AND** moving it does not trigger another CAD render

### Requirement: Apply or cancel placement explicitly

The window SHALL offer Apply and Cancel. Apply SHALL store the position of the new part as an Ecky translation in the active model. Rust SHALL check the expected version and source before saving. Cancel SHALL close the window and discard the preview.

#### Scenario: Apply the chosen position

- **GIVEN** a component is staged at finite XYZ coordinates and its destination source is current
- **WHEN** the user selects Apply
- **THEN** the model gains a single new version with a translated part at the chosen position
- **AND** the normal version workflow renders the new model once
- **AND** the placement window closes after a successful save and render

#### Scenario: Cancel before saving

- **GIVEN** a component is preparing or staged
- **WHEN** the user cancels or closes its window
- **THEN** the placement window and any preview disappear
- **AND** the project gains no source change or version

#### Scenario: The preview or apply fails

- **GIVEN** a placement in progress
- **WHEN** its preview fails, the destination source has changed, or the new version fails to render
- **THEN** the window shows the failure and keeps any entered coordinates visible
- **AND** a stale destination causes no source write
- **AND** a persisted version that failed to render remains available in normal version history
