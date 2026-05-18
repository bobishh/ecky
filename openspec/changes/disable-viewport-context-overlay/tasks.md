# Tasks: Disable Viewport Context Overlay

## 1. OpenSpec

- [x] 1.1 Add proposal.
- [x] 1.2 Add design.
- [x] 1.3 Add spec delta.

## 2. Outer BDD

- [x] 2.1 Update Playwright flows to assert viewport overlay stays absent.
- [x] 2.2 Keep Params editing coverage green for generated and imported models.
- [x] 2.3 Keep project-chooser/import happy paths green after selector drift fixes.

## 3. Implementation

- [x] 3.1 Add workbench-level kill switch for visible viewer context overlay.
- [x] 3.2 Leave hidden viewer overlay-disabled.

## 4. Verification

- [x] 4.1 Run `npm run typecheck`.
- [x] 4.2 Run targeted `npm run test:e2e -- e2e/qa.spec.ts ...`.
- [x] 4.3 Run browser proof on real route and capture screenshot.
- [x] 4.4 Run `cd src-tauri && cargo check`.
- [x] 4.5 Run `openspec validate disable-viewport-context-overlay`.
