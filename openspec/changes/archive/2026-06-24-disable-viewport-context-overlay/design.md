# Design: Disable Viewport Context Overlay

## Decision

Use a workbench-level kill switch instead of tearing out `Viewer.svelte`
overlay code.

Reason:

- lowest-risk change for current release.
- preserves selection/callout internals for later redesign.
- prevents geometry occlusion immediately.

## Ownership

- Workbench `App.svelte` decides whether visible viewer may render context
  overlay.
- `Viewer.svelte` keeps dormant overlay path behind prop gate.
- Params window remains sole visible parameter editing surface.

## Behavior Rules

- Main workbench viewer passes `showContextOverlay={false}` through a named
  flag.
- Hidden/background viewer already remains overlay-disabled.
- Part clicks still drive selection state and Params filtering.
- Tests must assert absence of `.viewer-part-overlay` in affected flows.

## Future Re-entry Rule

Do not re-enable overlay by flipping prop literals back to `true`.

Any future return requires:

- new OpenSpec change.
- clear non-occluding UX.
- Browser/Playwright proof on real workbench route.
