# Delta for visual-ast-authoring

## ADDED Requirements

### Requirement: Inline controls edit parameters through AST patches

The system SHALL let authors edit macro parameters in place on the visual AST
map through structured AST patches.

#### Scenario: Numeric parameter changes inline

- GIVEN the AST map shows a numeric parameter control
- WHEN the author changes the value through the inline control
- THEN the frontend sends a camelCase Tauri payload for a structured AST patch
- AND the Rust backend applies the corresponding snake_case patch operation
- AND serialized source contains the new parameter value
- AND preview updates from the accepted source.

#### Scenario: Invalid parameter shows raw backend error at node

- GIVEN the AST map shows a numeric parameter control
- WHEN the author enters a value the backend rejects
- THEN the source stays unchanged
- AND the control shows the raw backend/provider error body
- AND the diagnostic is anchored to the parameter node id.

#### Scenario: Existing parameter panel remains available during rollout

- GIVEN the author changes a parameter through the AST map
- WHEN the edit is accepted and preview updates
- THEN the covered flow does not open or depend on a separate parameter panel
- AND the existing parameter panel remains available as a separate fallback.

#### Scenario: Search result can be applied inline

- GIVEN the author has searched to a matching parameter node
- WHEN the author applies a valid value from the focused result
- THEN the backend applies a structured AST patch at that node
- AND the source stays canonical
- AND the focused region remains source-backed after reparse.

#### Scenario: SVG scene and HTML overlay stay aligned

- GIVEN the author views a source-backed macro map
- WHEN a parameter node is rendered
- THEN the structural blob, ports, and connectors appear in SVG
- AND the editable value control appears in an HTML overlay at the same
  control anchor
- AND focus, selection, and IME state stay on the HTML control, not the SVG
  paint layer.

### Requirement: Map supports click and typed insertion

The system SHALL let authors insert new source-backed macro nodes from selected
map regions.

#### Scenario: Click and type creates part

- GIVEN the author selects an empty insertion region inside a model or part
- WHEN the author types a recognized `part` intent
- THEN the backend validates the insertion position
- AND applies an AST patch that creates a part node
- AND the new part appears in the map
- AND serialized source contains the new part form.

#### Scenario: Click and type creates input

- GIVEN the author selects a valid input insertion region
- WHEN the author types a recognized parameter declaration
- THEN the backend applies an AST patch that creates an input or param node
- AND the new input appears as a port on the owning structure
- AND serialized source contains the new input declaration.

#### Scenario: Parameter edit remains compact in the scene

- GIVEN the author is looking at a part with multiple parameters
- WHEN the scene renders those values
- THEN the part stays grouped as a single visual module
- AND the parameters appear as compact attached readouts
- AND the view does not degrade into nested full-width cards.

#### Scenario: Unknown typed intent stays pending

- GIVEN the author selects an insertion point
- WHEN the author types text that cannot resolve to a valid macro construct
- THEN the pending insertion remains visible at that point
- AND the backend parser diagnostic is shown there
- AND source is not mutated.

### Requirement: Visual authoring enforces macro structure rules

The system SHALL enforce existing authoring constraints when map interactions
create new structures.

#### Scenario: Repeated physical structure uses repeat or instance

- GIVEN the author creates repeated shelves, ribs, clips, doors, or corridors
  from the map
- WHEN the backend applies the insertion patch
- THEN the authored source uses `repeat` or `instance`
- AND does not emit copy-pasted repeated shape blocks.

#### Scenario: Physical fit relation is named

- GIVEN the author creates a fit-critical relation from selected map or geometry
  nodes
- WHEN the backend applies the relation patch
- THEN the relation is represented by a named constraint or named binding
- AND no anonymous geometry offset is used for the fit-critical dimension.

#### Scenario: Boundary naming remains idiomatic

- GIVEN a visual authoring action crosses the Tauri boundary
- WHEN the frontend invokes the backend command
- THEN JavaScript argument names are camelCase
- AND Rust command arguments and structs are snake_case
- AND boundary structs use `#[serde(rename_all = "camelCase")]`.
