# Delta for ecky-semantic-ast

## ADDED Requirements

### Requirement: First-class construction groups

The `.ecky` language SHALL provide a group form usable inside a part's
binding sequence that names a section of bindings (e.g. derived dimensions,
construction profiles, structural solids, cutters, final assembly), survives
parsing as a named, nestable AST node with a stable id and byte span, keeps
binding scope sequential across group boundaries, and has no effect on
geometry evaluation or export output.

#### Scenario: Groups survive into the AST

- GIVEN a part whose bindings are organized into named groups
- WHEN the source is parsed
- THEN each group is an AST node carrying its name, span, and child bindings
- AND the block-tree projection receives group nodes without inferring
  sections from comments.

#### Scenario: Scope crosses group boundaries

- GIVEN a binding defined in a `dimensions` group
- WHEN a later `cutters` group references it
- THEN the reference resolves exactly as in a flat `let*`.

#### Scenario: Groups do not change geometry

- GIVEN a flat model and the same model re-organized into groups
- WHEN both are rendered and exported
- THEN the export digests are identical.

### Requirement: Explicit binding roles

The `.ecky` language SHALL let a binding declare a role from the closed set
`dimension`, `profile`, `solid`, `cutter`, `transform`, `boolean-result`,
`verification`; the declared role SHALL be present in the parsed AST before
any boolean assembly is analyzed, SHALL take precedence over name- or
usage-based inference, and a declared role contradicting observed usage
SHALL produce a diagnostic with the binding's span rather than a silent
re-classification.

#### Scenario: Cutter is known before assembly

- GIVEN a binding declared with role `cutter`
- WHEN the AST is inspected before the final boolean is reached
- THEN the node already carries the cutter role
- AND downstream consumers (UI, validators) need not derive it from
  `difference` operands.

#### Scenario: Declared role overrides inference

- GIVEN a binding named `helper-block` declared with role `cutter`
- WHEN roles are resolved
- THEN the declared cutter role wins over any name-based inference
- AND the node records that the role was declared, not inferred.

#### Scenario: Contradiction is a diagnostic

- GIVEN a binding declared `cutter` that is never used subtractively
- WHEN the model compiles
- THEN a diagnostic names the binding, its span, and the observed usage
- AND the declared role is not silently replaced.

#### Scenario: Unknown role is rejected

- GIVEN a binding declared with role `cuttter`
- WHEN the source compiles
- THEN compilation fails with the offending span and the allowed role set.

### Requirement: Node metadata annotations

The `.ecky` language SHALL support attaching metadata — at least label,
description, role, group, units, intended target, debug visibility, and
printability notes — to bindings, groups, and shape expressions; metadata
SHALL be preserved in the parsed AST and model manifests, SHALL be readable
by validators and UI, and SHALL be ignored by geometry evaluation, all
backend lowerings, and export digest computation. The existing model-level
`meta` clause SHALL be preserved under the same representation instead of
being dropped.

#### Scenario: Metadata reaches the manifest

- GIVEN a cutter binding annotated with a label and an intended target
- WHEN the model compiles
- THEN the manifest exposes the label and target on that node's id
- AND no lowering output contains the metadata.

#### Scenario: Metadata never changes geometry

- GIVEN a model with and without metadata annotations on its nodes
- WHEN both versions are exported
- THEN STL and STEP digests are identical.

#### Scenario: Model-level meta is no longer dropped

- GIVEN a model with a top-level `meta` clause
- WHEN the source is parsed
- THEN the meta content is available as model-level metadata in the AST
  and manifest.

### Requirement: Inspectable derived dimensions

Derived scalar bindings SHALL retain their formula as AST alongside the
evaluated value, including reference edges to each binding the formula
uses, so consumers can obtain the name, the formula, the current value, and
the definition site of every referenced identifier from the compiled model.

#### Scenario: Formula and value both survive compilation

- GIVEN `(outer-case-width (+ phone-pocket-width (* 2 side-wall-thickness)))`
- WHEN the model compiles
- THEN the projection exposes the formula structure, the evaluated value,
  and reference edges to `phone-pocket-width` and `side-wall-thickness`
- AND the evaluated number does not replace the formula in the AST.

#### Scenario: Reference edges are id-based

- GIVEN a derived dimension referencing another binding
- WHEN reference edges are read
- THEN each edge carries the stable node id of the defining binding.

### Requirement: Semantic forms obey the source identity contract

Every form added by this capability SHALL carry a stable node id and exact
byte span, SHALL round-trip through parse and emit-back unchanged, and
SHALL be addressable by the existing `ecky_ast_*` patch operations — this
covers groups, role and metadata annotations, semantic helpers, and
semantic checks — so the visual block view remains a one-to-one projection
of source.

#### Scenario: Emit-back round-trip

- GIVEN a model using groups, roles, metadata, helpers, and checks
- WHEN the source is parsed and emitted back
- THEN the emitted source is equivalent form-for-form to the input
- AND helper forms are emitted as authored, not as their desugared CSG.

#### Scenario: Patch addressing

- GIVEN a binding inside a nested group
- WHEN an `ecky_ast_*` patch targets it by path
- THEN the patch applies to exactly that binding's span
- AND the group structure around it is preserved.

#### Scenario: Existing models unaffected

- GIVEN a model using none of the semantic forms
- WHEN it compiles under this change
- THEN parse, render, verify, and export behavior is unchanged.
