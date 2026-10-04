## ADDED Requirements

### Requirement: Parameter-dependent flat-map retains symbolic inputs
The compiler SHALL support list-valued `flat-map` and `concat-map` callbacks over
parameter-dependent sequences without eager evaluation of symbolic parameters.

#### Scenario: Editable folded cord
- **GIVEN** a model with named dimensions and a count used by flat-map/range
- **WHEN** source compiles and native planning receives changed parameter values
- **THEN** its controls remain present and geometry uses those current values
- **AND** callbacks returning scalar values remain invalid.

### Requirement: Sequence sources preserve list intent
Expanded compilation SHALL parse source expressions in `map`, `zip`, `flat-map`,
`concat-map`, and `append` as sequences, including two- and three-number lists,
without changing Point2/Point3 inference in geometry positions.

#### Scenario: Three-number list is a map source
- **GIVEN** a map source `(list 0 0.5 1)` and a helper using a model parameter
- **WHEN** the model compiles
- **THEN** the source remains a three-item sequence and the parameter reference
  remains symbolic.

### Requirement: Failed fallback retains primary compiler diagnostic
The compiler SHALL retain a non-internal expanded-AST diagnostic when runtime
fallback also fails. A successful runtime fallback SHALL remain accepted.

#### Scenario: Invalid metadata does not become symbolic arithmetic error
- **GIVEN** invalid `meta` arity and parameter-dependent arithmetic
- **WHEN** both compiler paths reject the source
- **THEN** the result names the invalid `meta` clause rather than a numeric error
  produced by eager evaluation of a parameter symbol.

### Requirement: Unsupported quoted tuple mapping stays explicit
Quoted tuple data SHALL NOT be silently added as a new destructuring-map source.
The compiler SHALL return concise guidance naming supported `zip` or static
`enumerate` sources.

#### Scenario: Quoted tuple source is rejected clearly
- **GIVEN** a destructuring `map` whose source is quoted tuple data
- **WHEN** source compiles
- **THEN** compilation fails with `zip` and `enumerate` guidance
- **AND** the diagnostic contains no expanded AST dump.

### Requirement: Loft application uses documented signature
The compiler SHALL reject `apply` targeting `loft` and explain the direct
`loft distance profile1 profile2 ...` signature with explicit profile arguments.
Adding a distance to the profile list SHALL NOT make `apply loft` supported.

#### Scenario: Profile-list apply without distance
- **GIVEN** a model calling `(apply loft (append profiles))`
- **WHEN** expanded compilation rejects the unsupported call and runtime fallback
  also fails
- **THEN** the diagnostic explains that `loft` requires a leading distance
- **AND** it does not report an unrelated arithmetic error from a symbolic parameter.

### Requirement: Published loft reference matches compiler signature
The published surface reference SHALL document the compiler's explicit loft
distance and SHALL keep its shipped loft example compilable.

#### Scenario: Shipped loft example compiles
- **GIVEN** the published reference example uses a direct `loft` call
- **WHEN** the example is compiled by the Scheme source compiler
- **THEN** its explicit distance and profile arguments match the documented
  `loft distance profile1 profile2 ...` signature.

### Requirement: Repair preserves model signature
Repair SHALL retain user-visible controls and authored verification clauses.

#### Scenario: Restore spoon-rest controls
- **GIVEN** the existing CAD source has hardcoded dimensions after failed repair
- **WHEN** controls are restored and the current version is previewed
- **THEN** native CAD remains its source and exact version verification is recorded
- **AND** no imported STL replaces its editable source.

#### Scenario: Scheme header preserves current control schema
- **GIVEN** valid Ecky source begins with a single-semicolon comment
- **WHEN** the bound-source preview derives controls
- **THEN** it recognizes Ecky syntax and derives all current source parameters
- **AND** it does not retain an empty schema from the previous version.
