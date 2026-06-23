# component-surface Specification

## Purpose
TBD - created by archiving change component-unification. Update Purpose after archive.
## Requirements
### Requirement: Unified component entity

The authored surface SHALL represent `model`, `part`, `feature`, and
`define-component` forms as one component entity with a role
(`root`/`output`/`library`), a preserved spelling, an optional parameter
signature, and a body.

#### Scenario: Model parses as root component

- GIVEN an existing `.ecky` model authored with `(model (params ...) (part ...))`
- WHEN the source compiles
- THEN the resulting CoreProgram is identical to the pre-change output
- AND stable node keys are byte-identical to the pre-change derivation.

#### Scenario: Part parses as output component

- GIVEN a `(part id label expr)` clause
- WHEN the source compiles
- THEN the part produces exactly one CorePart with today's key/label/root
  behavior
- AND output-role components remain the only topology part boundaries.

### Requirement: Spelling preservation on emit

The emitter SHALL write each component back with its original authored
spelling.

#### Scenario: Roundtrip keeps spellings

- GIVEN source authored with `model`, `part`, and `feature` clause heads
- WHEN the source is parsed and re-emitted
- THEN every clause head re-emits with its original spelling
- AND no clause is rewritten to `component` or `define-component`.

### Requirement: Component definition and instantiation

The surface SHALL support `(define-component name (signature...) body)` and
instantiation `(name :key value ...)` with lexically scoped parameters,
defaults, and keyword overrides.

#### Scenario: Defaults and overrides

- GIVEN a component with signature entry `(number pin_d 8 :min 4 :max 12)`
- WHEN instantiated as `(knuckle)` and `(knuckle :pin_d 6)`
- THEN the first expansion binds `pin_d` to 8 and the second to 6.

#### Scenario: Closed body

- GIVEN a `define-component` body referencing a variable not in its signature
- WHEN the source compiles
- THEN compilation fails with an error naming the free variable and the
  component.

#### Scenario: Unknown keyword rejected

- GIVEN an instantiation passing a keyword not present in the signature
- WHEN the source compiles
- THEN compilation fails with an error naming the keyword and listing the
  component signature.

#### Scenario: Recursive instantiation rejected

- GIVEN components that instantiate themselves directly or in a cycle
- WHEN the source compiles
- THEN compilation fails with a deterministic cycle/depth error and does not
  hang.

### Requirement: Compile-time inline expansion

Component instantiation SHALL expand inline into the existing CoreProgram
shape before planning, with fresh node ids and the call site recorded as
source anchor.

#### Scenario: Core IR unchanged

- GIVEN a model whose parts are built from nested component instantiations
- WHEN the source compiles
- THEN the resulting CoreProgram uses only existing Core IR constructs
- AND `ecky_core_ir` public structs are unchanged by this feature.

#### Scenario: Both compile paths agree

- GIVEN the same component-using source
- WHEN compiled via the expanded-AST path and via the Steel runtime path
- THEN both produce identical CorePrograms.

### Requirement: Verify clauses travel with components

`verify` clauses authored inside a component definition SHALL expand once per
instantiation with tags namespaced by the instantiating part key.

#### Scenario: Per-instance verify tags

- GIVEN a component containing `(verify (tag fit) ...)` instantiated by parts
  `hinge_a` and `hinge_b`
- WHEN the model compiles and renders
- THEN structural verification reports checks tagged `hinge_a/fit` and
  `hinge_b/fit`.

