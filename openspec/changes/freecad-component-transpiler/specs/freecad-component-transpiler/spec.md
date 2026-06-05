# Delta for freecad-component-transpiler

## ADDED Requirements

### Requirement: FreeCAD parts are sourced by transpilation, not hand-authoring

The system SHALL produce stdlib components by translating parametric FreeCAD
documents (`.fcstd`) into parametric `.ecky`, and SHALL reuse the existing
component subsystem (`component_extract`, the versioned `component-library`,
`component_search`/`component_get`, and `component_import`) for everything
downstream of the transpiler. The transpiler is the only new authoring path; no
new storage or manifest format is introduced.

#### Scenario: A parametric FreeCAD part transpiles to parametric ecky

- GIVEN a `.fcstd` whose feature tree uses supported features (primitives,
  booleans, fillet/chamfer/thickness, sketch+extrusion, arrays)
- WHEN it is transpiled
- THEN a well-formed `(model (params …) (part …))` is emitted
- AND it compiles and renders on the default backend.

#### Scenario: Array features become parametric loops, not copied geometry

- GIVEN a `.fcstd` containing a `Part::Array` (e.g. a grid of features)
- WHEN it is transpiled
- THEN the array is emitted as a `repeat-union` (or array op) driven by a count,
  not as N hardcoded translated copies
- AND changing the count parameter changes the number of repeated features.

#### Scenario: Expression-bound dimensions become parameters

- GIVEN a `.fcstd` feature property bound to a spreadsheet cell or named
  expression
- WHEN it is transpiled
- THEN the referenced value is lifted into the `(params …)` block
- AND the dependent dimension is emitted as an ecky expression over that param,
  while unbound literal dimensions remain literal.

#### Scenario: A BREP-only part is not a component candidate

- GIVEN a `.fcstd` that contains only an imported solid with no parametric
  feature tree
- WHEN transpilation is attempted
- THEN it is rejected as a component candidate
- AND the diagnostic states it can only be brought in as a mesh/STEP import.

### Requirement: Transpiled parts become library components via the extract bridge

The system SHALL convert a transpiled parametric model into a closed
copy-inline `define-component` by running the existing `component_extract`
(`--save`) over it, such that the model's params become the component signature
and the component is persisted as a versioned package without bespoke storage
code.

#### Scenario: Transpile → extract → instantiate round-trip

- GIVEN a transpiled parametric model with a `(params …)` block
- WHEN `component_extract --save` is run on its part
- THEN a closed `define-component` whose signature matches the model params is
  saved under `component-library/<package_id>/<version>/`
- AND `component_get` returns its source
- AND instantiating it in a model (e.g. `(name :param value …)`) compiles,
  renders, and passes the component's own `verify` clauses on the default
  backend.

### Requirement: Parity target follows feature support across OCCT backends

The system SHALL verify a transpiled component against whichever OCCT backend
supports its features — native↔build123d when build123d implements the feature,
otherwise native↔FreeCAD — using a backend-agnostic STEP measurement to compare
bounding box and volume within tolerance.

#### Scenario: OCCT-only feature is parity-checked against FreeCAD

- GIVEN a component using an OCCT capability that build123d does not expose
- WHEN it is rendered native and on FreeCAD
- THEN both outputs are measured by importing their STEP into a common tool
- AND their bounding box and volume agree within tolerance.
