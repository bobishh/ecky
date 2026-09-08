## ADDED Requirements

### Requirement: Native examples are executable

The language catalogue SHALL publish `clip-plane` with quoted `:keep` text,
`clip-box` with required `:x`, `:y`, and `:z` ranges, and concrete Bézier
control points. The focused contract test SHALL compile each example in a model
and plan it through native Direct OCCT. The guidance SHALL preserve the observed
fact that a known bare `positive` literal currently plans, while an unresolved
local `positive` can fail name resolution.

#### Scenario: Published clipping examples

- **GIVEN** a native operation example from the catalogue
- **WHEN** the contract test inserts it unchanged into a minimal model
- **THEN** source compiles and native planning emits the expected operation.

### Requirement: Evidence claims stay bounded

Guidance SHALL identify native Bézier output as a fixed 16 samples per cubic
approximation. It SHALL identify topology checks
as insufficient for cross-section, shape intent, or support-free printing, and
require current artifact and viewport evidence for those claims.

#### Scenario: Structurally valid but flattened cord

- **GIVEN** an exported model with one component and no non-manifold edges
- **WHEN** an agent describes its round cross-section or support needs
- **THEN** guidance requires separate measurements and matching viewport evidence.

### Requirement: Authoring intent stays intact

Guidance SHALL require preservation of authored parameters and exact current
version evidence. A compiler error SHALL NOT be treated as permission to replace
CAD source with Python, STL, or hardcoded controls.

For local spacing edits, guidance SHALL preserve existing topology and design
intent, moving named spacing controls rather than filling loops or replacing
bent round profiles with solid teeth.

#### Scenario: Narrow existing folds

- **GIVEN** round folded geometry with named spacing controls
- **WHEN** a user requests closer spacing
- **THEN** guidance requires preserving round profiles and existing loops
- **AND** compiler failures do not authorize replacement with solid teeth or STL.
