## ADDED Requirements

### Requirement: Configure print instructions separately from geometry

Ecky SHALL read a project-local print setup with a slicer profile, selected rendered body IDs and filament slots, and optional layer events.

#### Scenario: Re-export an edited model

- **GIVEN** a project has a saved print setup and a new rendered version
- **WHEN** the user chooses 3MF in Export
- **THEN** the file contains the selected current production geometry and saved print instructions

#### Scenario: Reject an invalid setup

- **GIVEN** a setup references a missing body, invalid filament slot, or event outside the printable layers
- **WHEN** export is requested
- **THEN** Ecky reports the invalid entry and does not write a project

#### Scenario: Export without a print setup

- **GIVEN** a rendered model with one or more bodies and no saved print setup
- **WHEN** the user chooses 3MF in Export
- **THEN** the file contains all rendered bodies as geometry

### Requirement: Export a sliceable Orca project

Ecky SHALL export the selected geometry, printer and process settings, and typed layer events in an editable Orca 3MF.

#### Scenario: Print one solid with an insert and a different colour on top

- **GIVEN** a single rendered solid, a two-filament profile, a pause, and a later filament change
- **WHEN** the user exports it and slices the project in Orca
- **THEN** Orca schedules the pause and filament change on their selected layers and prints the model as one object

#### Scenario: Export multiple selected bodies

- **GIVEN** the setup selects multiple rendered bodies
- **WHEN** the user chooses 3MF in Export
- **THEN** the selected bodies retain their relative placement and assigned filament slots
