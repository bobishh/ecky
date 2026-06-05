# Proposal: Bottom Icon Dock

## Intent

Move the Ecky workbench navigation from the top-right text strip to a bottom
icon dock that reads like an Ecky CAD control rail, not a Cultiwars skin copy.

The current dock consumes the top-right viewport area, uses large text labels,
and exposes a standalone `+` action even though project creation already belongs
inside Projects. The target is a compact bottom rail with icon-first controls,
project-owned creation, and the existing Tactical Midnight visual language.

## Scope

- Reposition the workbench dock near the bottom center.
- Replace text tab labels with accessible icon controls.
- Remove the standalone `+` / new-project dock action.
- Make the dock `CODE` control toggle the inspector on repeated clicks.
- Move Sketch Workspace into a floating window instead of fullscreen mode.
- Remove viewport-side `CODE` from export actions; dock owns source access.
- Keep Projects as the owner of `+ NEW` / new project creation.
- Keep existing window toggle behavior for Projects, Params, Dialogue, Docs,
  Code, Sketch, audio, terminal, draw, and settings.
- Use Ecky-native visual direction: square borders, midnight glass, vertex/grid
  motifs, bronze/green accents.
- Add Playwright proof for dock placement, icon semantics, and Projects-owned
  new project flow.

## Out of Scope

- Rewriting floating window layout.
- Replacing ProjectSwitcher internals.
- Adding a separate agent status bar.
- Copying Cultiwars colors, paper texture, or hand-drawn artwork.
- Changing backend behavior or persistence.

## Proof Gates

- Workbench dock is visually located in the bottom band of the viewport.
- Dock buttons are icon-first but remain accessible by role/name.
- Standalone dock `+` action is absent.
- Dock `CODE` acts as a toggle.
- Sketch opens as a normal floating window and closes by the same toggle/window chrome.
- Projects window still exposes `+ NEW` and opens the global new-project chooser.
- Settings still opens and closes without hiding the workbench dock.
- One failure/pending state remains visible in the Projects window.
- `npm run test:e2e -- e2e/app.spec.ts` targeted checks pass.
- `cd src-tauri && cargo check` passes before completion report.
