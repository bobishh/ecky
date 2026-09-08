## ADDED Requirements

### Requirement: Parameter-dependent flat-map retains symbolic inputs
The compiler SHALL support list-valued `flat-map` and `concat-map` callbacks over
parameter-dependent sequences without eager evaluation of symbolic parameters.

#### Scenario: Editable folded cord
- **GIVEN** a model with named dimensions and a count used by flat-map/range
- **WHEN** source compiles and native planning receives changed parameter values
- **THEN** its controls remain present and geometry uses those current values
- **AND** callbacks returning scalar values remain invalid.

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
