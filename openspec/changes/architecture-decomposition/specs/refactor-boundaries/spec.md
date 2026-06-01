# Delta for refactor-boundaries

## ADDED Requirements

### Requirement: Contract module split preserves public shape

The system SHALL split the Rust Tauri contract surface by domain without
changing public Rust import paths or generated TypeScript contract shape.

#### Scenario: Contract re-exports preserve callers

- GIVEN contract types are moved into domain modules
- WHEN Rust modules import types through `crate::contracts::*` or
  `crate::contracts::TypeName`
- THEN those imports still resolve through `contracts/mod.rs` re-exports
- AND no caller needs a path change.

#### Scenario: Generated frontend contracts stay byte-stable

- GIVEN the contract split is complete
- WHEN TypeScript contracts are regenerated
- THEN `src/lib/tauri/contracts.ts` has no behavioral shape diff
- AND frontend Tauri payload names remain camelCase.

### Requirement: MCP handlers split by domain

The system SHALL split MCP tool handlers into cohesive domain modules while
preserving the exposed tool list and behavior.

#### Scenario: Handler tests live with handler domain

- GIVEN a handler moves out of the monolithic handlers file
- WHEN its tests are moved
- THEN those tests live beside the domain handler module
- AND the same handler behavior remains covered.

#### Scenario: Tool dispatch is registry-owned

- GIVEN MCP tool handlers are split by domain
- WHEN a new tool is added
- THEN dispatch requires one registry entry and one domain handler function
- AND the tool definition list remains unchanged for existing tools.

### Requirement: Backend capability tax becomes explicit

The system SHALL record native OCCT as the primary geometry backend and make
text-backend coverage explicit before any backend removal.

#### Scenario: Native backend coverage is complete or explicit

- GIVEN every CoreOperation is enumerated
- WHEN backend capabilities are checked
- THEN native OCCT covers each operation or marks it explicitly unsupported
- AND build123d and FreeCAD declare only their verified subsets.

#### Scenario: Backend demotion does not remove behavior in this change

- GIVEN build123d and FreeCAD are documented as export or interop paths
- WHEN architecture decomposition work runs
- THEN no build123d or FreeCAD render path is removed
- AND later removal requires a separate change.

### Requirement: Frontend Tauri calls stay behind client wrapper

The system SHALL keep Svelte components from invoking Tauri commands directly.

#### Scenario: Docs EPUB export uses client wrapper

- GIVEN the docs site runs inside a Tauri invoke bridge
- WHEN the user downloads the EPUB through the native save dialog
- THEN `DocsSite.svelte` calls a function from `tauri/client`
- AND the wrapper sends `export_docs_book_epub` with a camelCase `targetPath`
  payload.

#### Scenario: Browser docs download stays local

- GIVEN the docs site runs without a Tauri invoke bridge
- WHEN the user downloads the EPUB
- THEN the browser fallback download path still runs
- AND no native Tauri command is invoked.

### Requirement: ParamPanel keeps map extraction shell only

The system SHALL extract the New Params macro-AST map from `ParamPanel.svelte`
without changing user-visible selectors or workbench theme constraints.

#### Scenario: Extracted map preserves visible shell

- GIVEN the New Params tab is open
- WHEN the macro-AST map renders from the extracted component
- THEN `.macro-ast-map-shell` and macro source pane selectors remain available
- AND Tactical Midnight square-border styling and overflow boundaries remain
  enforced.

#### Scenario: ParamPanel remains tab owner

- GIVEN the macro-AST map is extracted
- WHEN ParamPanel renders tabs
- THEN ParamPanel owns the tab shell and mode switching
- AND map projection, camera, minimap, and source-pane wiring live outside the
  tab shell.
