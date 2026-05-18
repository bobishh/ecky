# Proposal: Disable Viewport Context Overlay

## Intent

Temporarily disable the in-viewport parameter/context overlay in Ecky
workbench.

Current behavior is wrong on two fronts:

- overlay appears even when no useful controls exist.
- overlay covers geometry and degrades viewport inspection.

User direction is explicit: stop rendering this UI for now, keep parameter
editing in Params, and prevent spec/test drift from reintroducing the overlay
by accident.

## Scope

- Disable viewport context overlay in main workbench viewer.
- Keep part selection and Params window flows working.
- Rewrite overlay-facing Playwright expectations toward Params ownership.
- Add OpenSpec requirement that overlay stays off until redesign.

## Out of Scope

- Full deletion of dormant overlay implementation inside `Viewer.svelte`.
- Redesign of contextual editing UX.
- Changes to param semantics, selection mapping, or backend rendering.

## Proof Gates

- Workbench viewer never renders `.viewer-part-overlay` during normal param
  editing flows.
- Selecting parts still exposes editable controls in Params.
- Imported FCStd bindings remain editable from Params.
- New-project chooser/import regression checks stay green after test rewrites.
- `npm run typecheck` passes.
- `cd src-tauri && cargo check` passes before completion report.
