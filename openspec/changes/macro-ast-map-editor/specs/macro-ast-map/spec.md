# Delta for macro-ast-map

## ADDED Requirements

### Requirement: Macro source projects to additive params map

The system SHALL project `.ecky` macro source into a structured `New Params`
visual map without making the visual map the source of truth or removing the
existing parameter panel.

#### Scenario: Existing macro renders in New Params

- GIVEN an existing `.ecky` macro with a model, nested part, input param, and
  numeric parameter
- WHEN the author opens `New Params`
- THEN the editor shows the model as the root structure
- AND the nested part is visible as a child structure
- AND the input param is visible as a port
- AND the numeric parameter has an inline control anchor
- AND the existing parameter panel entrypoint remains available.

#### Scenario: Source remains canonical

- GIVEN a macro AST map is visible
- WHEN the source is parsed, projected to map, serialized, and reparsed
- THEN the reparsed program preserves the same authored macro semantics
- AND no renderer-only state is required to recover the source.

### Requirement: AST nodes have stable identity

The system SHALL assign stable ids to map nodes so selection, patches,
verification links, preview results, and diagnostics can target source-backed
entities.

#### Scenario: Formatting change preserves node ids

- GIVEN `.ecky` source with stable model, part, input, and param forms
- WHEN whitespace, indentation, or comments change without semantic edits
- THEN the same source-backed entities keep the same map node ids.

#### Scenario: Unrelated branch edit preserves node selection

- GIVEN an author has selected a parameter node in one branch
- WHEN another branch receives a valid AST patch
- THEN the selected parameter keeps its node id
- AND selection remains on that parameter.

#### Scenario: Ambiguous identity reports diagnostic

- GIVEN a source edit creates two unnamed sibling forms that cannot receive
  deterministic distinct semantic ids
- WHEN the backend projects the AST map
- THEN the response includes a deterministic identity diagnostic
- AND the frontend can display that diagnostic at the affected parent node.

### Requirement: Map renderer follows workbench UI boundaries

The system SHALL render the AST map inside existing workbench theme and layout
constraints.

#### Scenario: Tactical map shell renders

- GIVEN the AST map editor is open
- WHEN the map shell renders
- THEN it uses Tactical Midnight theme colors
- AND square borders
- AND `--primary` or `--secondary` bronze accents for selected or active map
  elements
- AND uses futuristic blob, glow, or port styling without locking the product to
  literal molecular biology.

#### Scenario: Map containers constrain overflow

- GIVEN a macro map contains many nested structures or long labels
- WHEN the viewport renders on desktop or mobile size
- THEN major map containers keep `overflow: hidden`
- AND controls or labels do not bleed into unrelated workbench regions.

### Requirement: Scene uses layered SVG and HTML

The system SHALL render the macro map as an SVG-led structural scene with HTML
controls overlaid at shared layout coordinates.

#### Scenario: Structural layer stays separate from controls

- GIVEN `New Params` renders a source-backed macro map
- WHEN the scene is drawn
- THEN SVG renders the structural nodes, ports, connectors, focus rings, and
  glow-safe shapes
- AND the scene reflects syntax types visually through node variants, badges,
  or shape cues for model, part, port, and value nodes
- AND HTML renders the interactive inputs, buttons, and search result anchors
  on top of the same layout model
- AND canvas, if present, only serves decoration or background underlay.

#### Scenario: Compact modules wrap around parent parts

- GIVEN a macro map shows several numeric parameters under one part
- WHEN the scene lays out those parameters
- THEN each parameter appears as a compact module attached to the owning part
- AND the scene does not expand the entire stack into full-width rows
- AND the owning part remains visually grouped as a single mechanism blob.

### Requirement: Search focuses map regions

The system SHALL use search in `New Params` as spatial navigation to
source-backed map regions.

#### Scenario: Parameter search focuses owning region

- GIVEN `New Params` shows several source-backed parameter controls
- WHEN the author searches for a parameter by name or visible label
- THEN matching results are listed
- AND choosing a result selects the matching node
- AND the map focuses or frames the owning region.

#### Scenario: Find then apply keeps source-backed identity

- GIVEN `New Params` shows a matching parameter node after search
- WHEN the author applies a valid inline edit from that focused result
- THEN the backend applies a structured AST patch at the selected node
- AND the node keeps its stable id if semantics did not change
- AND the updated source can be reparsed into the same map region.

#### Scenario: No-match search preserves state

- GIVEN `New Params` has a selected parameter node
- WHEN the author searches for a string with no matches
- THEN the view shows a no-match state
- AND source remains unchanged
- AND the current selection is not mutated.
