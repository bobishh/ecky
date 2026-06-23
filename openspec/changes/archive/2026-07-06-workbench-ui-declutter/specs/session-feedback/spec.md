# Delta for session-feedback

## ADDED Requirements

### Requirement: Errors surface through the Ecky bubble only

The workbench SHALL surface error state through the Ecky bubble and SHALL NOT
render a separate error banner. Session-level errors (render, export, config,
import) SHALL reach the bubble so no error is lost when the banner is removed.

#### Scenario: No standalone error banner

- GIVEN any error state is active
- WHEN the workbench renders
- THEN no `.error-banner` element is present.

#### Scenario: Session error appears in the bubble

- GIVEN a session-level error is set (e.g. a render or export failure)
- WHEN the bubble presentation resolves
- THEN the bubble carries the error text.
