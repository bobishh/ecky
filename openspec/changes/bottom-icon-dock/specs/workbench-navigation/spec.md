## ADDED Requirements

### Requirement: Bottom Icon Workbench Dock

The system SHALL render workbench navigation as a bottom-positioned icon dock
using Ecky Tactical Midnight visual language.

#### Scenario: Dock appears at bottom

- GIVEN the workbench route is loaded
- WHEN the workbench navigation renders
- THEN the dock is positioned in the bottom half of the viewport
- AND the dock remains inside the viewport bounds

#### Scenario: Dock controls are accessible

- GIVEN the dock renders icon-first controls
- WHEN assistive queries read the controls by role and name
- THEN Projects, Parameters, Dialogue, Ecky IR docs, Code inspector, Sketch
  Workspace, audio, draw, and settings controls are available

### Requirement: Project-Owned Creation

The system SHALL NOT expose standalone new-project creation in the workbench
dock when the Projects window already owns the `+ NEW` action.

#### Scenario: Dock has no standalone plus

- GIVEN the workbench dock is visible
- WHEN the dock controls are inspected
- THEN no dock button named `+` or `New project` is present

#### Scenario: Projects still creates new projects

- GIVEN the Projects window is open
- WHEN the user activates `+ NEW`
- THEN the global `Start New Project` chooser opens
- AND the chooser is not nested inside the Projects window

### Requirement: Navigation State Preservation

The system SHALL preserve existing window toggle behavior while changing dock
presentation.

#### Scenario: Settings round trip keeps dock visible

- GIVEN the workbench dock is visible
- WHEN Settings opens and closes
- THEN the dock remains visible
- AND Parameters remains available by accessible name
