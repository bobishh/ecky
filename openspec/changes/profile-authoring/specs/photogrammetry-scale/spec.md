## ADDED Requirements

### Requirement: Photogrammetry dimensions use millimeters exactly once

The system SHALL convert Object Capture meter coordinates to millimeters once at the reconstruction boundary and SHALL treat downstream scan bounds and sections as millimeters without reapplying preview scaling. Existing calibration inputs and provenance remain authoritative; this change does not add a new calibration editor.

#### Scenario: Reconstruct meter-valued capture
- **GIVEN** Object Capture geometry whose measured extent is one meter
- **WHEN** reconstruction produces profile-capable mesh bounds
- **THEN** the reported extent is 1000 mm and downstream section/authoring consumes that value without a second scale conversion

#### Scenario: Read an existing capture run
- **GIVEN** a previously saved capture run with a preview scale value
- **WHEN** the run is reopened after the scale correction
- **THEN** its stored scale is preserved and interpreted compatibly rather than silently rewritten

#### Scenario: Use existing calibration
- **GIVEN** a capture run with an existing explicit calibration
- **WHEN** its mesh is sectioned for profile authoring
- **THEN** the corrected millimeter scale and existing calibration provenance are retained separately from approximate mesh accuracy

#### Scenario: Use an uncalibrated scan
- **GIVEN** a scan without explicit calibration
- **WHEN** it is sectioned for profile authoring
- **THEN** the profile evidence is labeled approximate and the UI does not imply verified dimensions

## ROADMAP Requirements (Later Releases)

These capabilities are not first-release acceptance criteria:

- A dedicated scan calibration editor or a new point-to-point reference measurement flow.
- Higher-quality photogrammetry reconstruction with stronger diagnostics and repair suggestions.
