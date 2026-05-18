## ADDED Requirements

### Requirement: Viewport Context Overlay Disabled

The system SHALL keep the main workbench viewport free of the in-view parameter
overlay until a replacement UX is explicitly specified and implemented.

#### Scenario: Part selection does not spawn viewport controls

- GIVEN a rendered model in the workbench
- WHEN the user selects a part from Params or by clicking the model
- THEN `.viewer-part-overlay` is not rendered
- AND editable controls remain available from Params

#### Scenario: Imported bindings stay editable without viewport overlay

- GIVEN an imported FCStd model with accepted semantic bindings
- WHEN the user edits a bound value
- THEN the edit happens from Params
- AND the viewport remains unobscured by the parameter overlay
