# Delta for agent-visibility

## ADDED Requirements

### Requirement: Bubble opens full session activity

The system SHALL make the advisor bubble clickable so the user can inspect the
full related event.

#### Scenario: Long agent report is truncated in bubble

- GIVEN an agent report exceeds compact bubble space
- WHEN the user clicks the bubble body
- THEN the session activity window opens
- AND the full report text is visible without truncation.

#### Scenario: Copy and dismiss remain independent

- GIVEN the bubble has copy and dismiss controls
- WHEN the user clicks copy or dismiss
- THEN the bubble does not also open the activity window
- AND the existing copy or dismiss behavior remains intact.

### Requirement: Agent actions are visible

The system SHALL show agent actions that affect design state as visible activity
items.

#### Scenario: Agent changes macro

- GIVEN an agent changes macro source
- WHEN the change reaches the app
- THEN the activity list shows an agent macro change item
- AND the item links to the macro diff.

#### Scenario: Agent changes parameters

- GIVEN an agent changes model parameters
- WHEN the change reaches the app
- THEN the activity list shows an agent parameter change item
- AND the item shows old and new values.

#### Scenario: Agent validation report arrives

- GIVEN an agent reports validation issues
- WHEN the report reaches the app
- THEN the activity list shows validation status
- AND the detail view includes every issue and raw report text.

### Requirement: Preview details are inspectable

The system SHALL let the user open extended preview details from bubble or
activity.

#### Scenario: Preview event has image

- GIVEN a preview event includes image data
- WHEN the user opens the event detail
- THEN the detail view displays the preview image
- AND related artifact metadata is visible.

#### Scenario: Preview event has validation issues

- GIVEN a preview event links to validation issues
- WHEN the user opens the preview detail
- THEN validation summary and issue list are visible next to the preview.
