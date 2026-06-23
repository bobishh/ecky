# session-collaboration Specification

## Purpose
TBD - created by archiving change session-collab-visibility. Update Purpose after archive.
## Requirements
### Requirement: Session event log for visible collaboration

The system SHALL record user, agent, and system actions that affect the active
design as session events.

#### Scenario: Agent proposes a macro patch

- GIVEN an agent produces a macro change
- WHEN the patch is accepted into the working design or preview draft
- THEN the session event log contains a macro patch event
- AND the event records actor, timestamp, summary, old source reference, and new
  source reference.

#### Scenario: User changes parameters

- GIVEN the user applies parameter changes
- WHEN render starts from those parameters
- THEN the session event log contains a params changed event
- AND the event includes old and new values for changed keys.

#### Scenario: System renders a model

- GIVEN a render is requested
- WHEN rendering starts and completes
- THEN the session event log contains render started and render succeeded events
- AND the success event links to the runtime artifact bundle.

#### Scenario: Render fails

- GIVEN a render fails
- WHEN the backend returns an error
- THEN the session event log contains a render failed event
- AND the event includes the raw backend error detail.

### Requirement: Session projections drive collaboration UI

The system SHALL derive bubble, activity, preview, and code-diff UI state from
session events.

#### Scenario: Bubble chooses important event

- GIVEN multiple session events exist
- WHEN the bubble projection runs
- THEN it selects the newest active warning, error, question, or agent action
  according to deterministic priority.

#### Scenario: Activity filters by active work

- GIVEN events from multiple threads or versions exist
- WHEN the session activity window opens from the active workbench
- THEN the visible activity defaults to events related to the active thread
- AND the user can inspect event detail without losing current viewport state.

### Requirement: Session state migration is incremental

The system SHALL keep existing stores operational while session events become the
visibility layer.

#### Scenario: Existing manual apply path runs

- GIVEN the manual code apply flow works before this change
- WHEN session event emission is added
- THEN the existing render, working copy, history, and param panel behavior
  still works
- AND a corresponding session event is emitted.

