# Tasks: Bottom Icon Dock

## 1. OpenSpec

- [x] 1.1 Add proposal.
- [x] 1.2 Add design.
- [x] 1.3 Add spec delta.

## 2. Outer BDD

- [x] 2.1 Add Playwright test for bottom icon dock accessibility and placement.
- [x] 2.2 Add Playwright test proving standalone dock `+` is absent.
- [x] 2.3 Keep Projects `+ NEW` chooser test green.
- [x] 2.4 Add failure/pending Projects state assertion.

## 3. Implementation

- [x] 3.1 Replace text dock content with icon-first markup.
- [x] 3.2 Remove standalone dock `+` action.
- [x] 3.3 Move overlay actions to bottom-center workbench rail.
- [x] 3.4 Restyle rail with Ecky Tactical Midnight language, without decorative background patterns.
- [x] 3.5 Keep onboarding target attributes and active window states.
- [x] 3.6 Keep utility controls grouped and separated.
- [x] 3.7 Add responsive guard for narrow viewport.
- [x] 3.8 Make dock `CODE` a repeated-click toggle.
- [x] 3.9 Render `SKETCH` as a floating window and remove in-workspace close action.
- [x] 3.10 Remove duplicate viewport export-bar `CODE`.

## 4. Verification

- [x] 4.1 Run targeted Playwright spec.
- [x] 4.2 Run `npm run typecheck`.
- [x] 4.3 Run `cd src-tauri && cargo check`.
- [x] 4.4 Use browser proof on real route after implementation.
