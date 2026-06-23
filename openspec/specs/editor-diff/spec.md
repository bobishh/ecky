# editor-diff Specification

## Purpose
TBD - created by archiving change session-collab-visibility. Update Purpose after archive.
## Requirements
### Requirement: Macro editor shows latest diff

The system SHALL show the latest macro change diff in the code editor window.

#### Scenario: Agent macro patch exists

- GIVEN an agent macro patch event exists
- WHEN the user opens the code editor
- THEN the editor shows current code in CodeMirror
- AND a labeled `LAST MACRO DIFF` section shows changed lines.

#### Scenario: User applies manual code

- GIVEN the user applies code in the macro editor
- WHEN the apply succeeds
- THEN a macro patch event is recorded
- AND reopening the code editor shows the applied change in the diff section.

#### Scenario: No macro diff exists

- GIVEN no macro patch event exists for the active design
- WHEN the user opens the code editor
- THEN the diff section is absent or shows an empty state
- AND the editor remains usable.

### Requirement: Parameter changes show old and new values

The system SHALL show parameter diffs for parameter change events.

#### Scenario: Width parameter changes

- GIVEN parameter `width` changes from `100` to `80`
- WHEN the params changed event is shown
- THEN the detail view includes `width: 100 -> 80`.

#### Scenario: Multiple parameters change

- GIVEN multiple parameter keys change
- WHEN the params changed event is shown
- THEN every changed key is listed once
- AND unchanged keys are omitted.

