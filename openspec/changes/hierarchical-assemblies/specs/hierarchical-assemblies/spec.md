## ADDED Requirements

### Requirement: Dedicated Assembly tab in Parameters

The Parameters workspace SHALL expose hierarchical assembly navigation in a dedicated `ASSEMBLY` tab rather than inline above parameter fields.

#### Scenario: Switch between Parameters and Assembly tabs on a multi-part model

- **GIVEN** a rendered multi-part model with an assembly tree
- **WHEN** the user views the `PARAMETERS` tab
- **THEN** parameter controls render without an inline assembly tree
- **AND** selecting the `ASSEMBLY` tab renders the full-height assembly tree with sub-assembly expand/collapse, part selection, Solo isolation, and hide/show controls
- **AND** switching to a single-part model without isolation hides the `ASSEMBLY` tab and falls back to `PARAMETERS`

### Requirement: Viewport part isolation, visibility, and parameter scoping

The workbench SHALL let users Solo-isolate or hide parts and sub-assemblies in the 3D viewport and scope parameter controls to the isolated part.

#### Scenario: Solo isolate a bulb and scope parameters

- **GIVEN** the two-bulb-and-stand assembly (`left`, `right`, `stand`) after display-tier Apply or production render
- **WHEN** the user clicks Solo on `left` in the `ASSEMBLY` tab
- **THEN** the viewport isolates `left`, ghosts or hides sibling parts per toggle, and frames `left`'s own bounding box
- **AND** switching to `PARAMETERS` displays the isolated scope bar filtering parameters to `left` with a `SHOW ALL` override
- **AND** pressing Escape or Exit restores the full assembly view
