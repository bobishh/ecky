# Delta for workbench-viewport

## REMOVED Requirements

### Requirement: Direct OCCT STEP status overlay

**Reason:** The success badge restates state the user does not act on; failures
are reported through the error surface. Removing it declutters the viewport.

**Migration:** None. The `directOcctStepStatus` derivation and its overlay are
removed; export availability is still gated by `canExportModel`.
