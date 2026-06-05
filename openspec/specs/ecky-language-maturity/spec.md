# ecky-language-maturity Specification

## Purpose
TBD - created by archiving change language-maturity. Update Purpose after archive.
## Requirements
### Requirement: Verify-TDD Generation Loop
Generated CAD responses SHALL author model geometry and top-level `(verify ...)`
clauses together, run authored verification after render, and retry red results
within the configured retry cap before presenting the version.

#### Scenario: Red authored verify retries to green
- GIVEN generation returns a model with authored verify clauses
- AND the first rendered version fails structural or authored verification
- WHEN retry attempts remain
- THEN the next generation prompt includes deterministic verification feedback
- AND the failed version is not committed as a successful result
- AND a later passing verification commits the green result

#### Scenario: MCP agent verifies before commit
- GIVEN an MCP agent creates or edits a `.ecky` model through preview tools
- WHEN the preview may become a user-visible version
- THEN the agent guidance requires `verify_generated_model` before commit
- AND red verification drives source or parameter repair before another preview
- AND a capped red result is reported honestly instead of committed silently

#### Scenario: Retry cap preserves honest red result
- GIVEN generation returns a model with authored verify clauses
- AND verification remains red after the retry cap
- WHEN the app presents the final result
- THEN the response reports the verification failure explicitly
- AND the failure details remain visible to the user or agent

### Requirement: Unit Literals And Dimension Metadata
The `.ecky` compiler SHALL accept suffixed numeric literals for length and
angle units, normalize them to canonical numeric values, and preserve enough
dimension metadata for params, UI chips, and verifier diagnostics.

#### Scenario: Suffixed literals normalize
- GIVEN `.ecky` source contains `70mm`, `1cm`, `1in`, `45deg`, or `0.5rad`
- WHEN the source compiles
- THEN length values are normalized to millimeters
- AND angle values are normalized to degrees

#### Scenario: Strict unit mismatch fails
- GIVEN `.ecky` source opts into strict units
- AND a length-valued parameter is used where an angle is required
- WHEN the verifier checks the program
- THEN compilation fails with the offending operation, span, expected dimension,
  and actual dimension

### Requirement: Stable Authored Selectors
The `.ecky` language SHALL support authored face and edge tags that resolve to
stable target ids, persist in the model manifest, and can be reused wherever
selector strings are currently accepted.

#### Scenario: Tagged selector survives parameter sweep
- GIVEN a fillet targets a face through a named authored tag
- WHEN a parameter change reorders backend face indices
- THEN the fillet still targets the intended tagged face
- AND the manifest records the tag name, authored selector, and durable ids

#### Scenario: Provenance selector filters by shape binding
- GIVEN a build expression creates named intermediate shapes
- WHEN a selector uses `:created-by <shape>`
- THEN candidate faces or edges are filtered to topology produced by that shape

### Requirement: Preview Views Stay Out Of Manufacturing Geometry
The `.ecky` language SHALL support named preview-only views that apply display
transforms without changing STL or STEP export geometry.

#### Scenario: Exploded view leaves export digest unchanged
- GIVEN a model contains a named view with per-part offsets
- WHEN exports are produced with and without that view
- THEN manufacturing artifact digests are identical
- AND the view data is present only in preview/display metadata

### Requirement: CAD Diagnostics Include Physical Context
Normalize, plan, render, export, and verify failures SHALL include physical
context: part key, operation name, source span when available, and resolved
parameter values involved in the failure.

#### Scenario: Failure names live parameter values
- GIVEN a boolean or verify failure depends on model parameters
- WHEN the failure is reported
- THEN the diagnostic includes the responsible part, operation or metric, source
  location when known, and live parameter values

### Requirement: Reuse Guidance And Compiler Support
The language guide and authoring card SHALL direct repeated or derived geometry
toward `let*`, helper `define`, `define-component`, `repeat`, or `instance`, and
compiler gaps found while documenting those patterns SHALL be fixed additively.

#### Scenario: Repeated structure is documented as one intent
- GIVEN a book-scale model repeats derived dimensions or repeated parts
- WHEN the authoring guide explains the model
- THEN derived values appear once through named bindings
- AND repeated geometry is expressed through reusable language forms

