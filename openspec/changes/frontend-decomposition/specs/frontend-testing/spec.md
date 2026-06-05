# Delta for frontend-testing

## ADDED Requirements

### Requirement: Component-test layer exists between unit and e2e

The frontend SHALL have a component-test harness that renders individual Svelte
components in isolation, sitting between pure-function unit tests and full-app
e2e.

#### Scenario: Harness runs in the test pipeline

- GIVEN the repository test scripts
- WHEN the frontend test suite runs in CI
- THEN a component-test runner (e.g. vitest + @testing-library/svelte) executes
- AND it can mount a single component with props and assert on its rendered DOM
  without launching the full Tauri app.

### Requirement: UI presence and wiring assertions live in component tests, not e2e

The system SHALL express single-component assertions (element presence, labels,
disabled state, prop-driven branches) as component tests, and SHALL reserve
full-app e2e for cross-domain flows.

#### Scenario: Extracted seam ships with component tests

- GIVEN a component extracted during a decomposition slice
- WHEN the slice is implemented
- THEN the component has component tests covering its rendering/wiring contract
- AND presence/label/disabled-state assertions for that component are removed
  from the full-app e2e specs (or never added there).

#### Scenario: e2e keeps only cross-domain coverage

- GIVEN an e2e spec touching an extracted component
- WHEN it is updated for the slice
- THEN it asserts only behavior that spans components or the Tauri boundary
- AND single-component presence checks have moved to component tests.
