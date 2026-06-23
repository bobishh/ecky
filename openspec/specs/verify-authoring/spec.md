# verify-authoring Specification

## Purpose
TBD - created by archiving change verify-authoring-foundation. Update Purpose after archive.
## Requirements
### Requirement: Top-level verify clauses parse under model

The system SHALL accept a top-level `(verify ...)` clause in `.ecky` source so
authors can declare verification metadata beside geometry authoring.

#### Scenario: Top-level verify parses

- GIVEN `.ecky` source with geometry and a top-level `(verify ...)` clause
- WHEN the source is parsed and compiled
- THEN the verify clause is preserved in authored program constraints
- AND geometry compilation continues through existing preparation flow.

#### Scenario: Nested verify is rejected

- GIVEN `.ecky` source that places `(verify ...)` inside a geometry or
  expression form
- WHEN the source is parsed and compiled
- THEN compilation fails
- AND the diagnostic identifies `verify` as unsupported in that nested position.

#### Scenario: Empty verify is rejected

- GIVEN `.ecky` source with `(verify)` and no required sections
- WHEN the source is parsed and compiled
- THEN compilation fails
- AND the diagnostic states that `tag`, `metric`, and `expect` sections are
  required.

### Requirement: Verify clauses preserve tag metric and expect payload

The system SHALL preserve authored `tag`, `metric`, and `expect` payload as
opaque data in core IR.

#### Scenario: Verify payload preserves authored values

- GIVEN a verify clause containing symbols, strings, booleans, numbers, and
  nested list forms
- WHEN the source is compiled
- THEN the authored section payload is preserved in order
- AND nested list shape remains intact.

#### Scenario: Verify clause roundtrips through emit

- GIVEN source with a top-level verify clause
- WHEN the source is compiled, emitted back to legacy source, and reparsed
- THEN the emitted source still contains the verify clause
- AND the reparsed program preserves the same `tag`, `metric`, and `expect`
  payload.

### Requirement: Structural verification evaluates authored verify additively

The system SHALL evaluate authored verify clauses additively in structural
verification entrypoints that already consume runtime bundles without replacing
the existing structural result carrier.

#### Scenario: Authored verify failure merges into structural verification

- GIVEN an Ecky-generated model with readable authored source and a verify
  clause whose expectation fails against runtime evidence
- WHEN generated-model verification runs
- THEN the returned structural verification result reports `passed = false`
- AND an authored verification failure is appended beside structural issues
- AND existing artifact digest reporting remains intact.

#### Scenario: Unsupported authored metric reports deterministic error

- GIVEN an Ecky-generated model with readable authored source and a verify
  clause that references an unsupported metric namespace or key
- WHEN generated-model verification runs
- THEN the request succeeds without panic
- AND an authored verification error is reported through the structural issue
  list.

#### Scenario: Summary response reflects authored verify additions

- GIVEN an Ecky-generated model whose authored verify clause fails
- WHEN MCP structural verification summary runs
- THEN the summary response reports `passed = false`
- AND `issueCount` includes authored verification additions
- AND summary text reflects the merged structural issue codes.

### Requirement: Code inspector can seed authored verify template

The system SHALL let authors append one starter verify clause from the existing
code inspector when the visible source is a model-shaped `.ecky` buffer.

#### Scenario: Insert verify template into model source

- GIVEN the code inspector shows model-shaped `.ecky` source without a verify
  clause
- WHEN the author triggers verify insertion
- THEN the visible source gains one top-level verify template before the model
  close
- AND the inspector reports that verify insertion succeeded.

#### Scenario: Duplicate verify insertion stays blocked

- GIVEN the code inspector shows model-shaped `.ecky` source that already
  contains a verify clause
- WHEN the author views code inspector actions
- THEN the verify insertion action is disabled
- AND existing authored verify source stays unchanged.

#### Scenario: Docs-opened snippet stays scratch-only

- GIVEN the docs window opens a snippet inside the existing code inspector
- WHEN the snippet modal is visible
- THEN apply, fork, and commit version actions are not available
- AND verify insertion action is not available
- AND source-mode or scratch-status badges are not shown
- AND the snippet remains local scratch content instead of a live version edit.

#### Scenario: Version-mode apply keeps inserted verify source

- GIVEN the workbench code inspector opens real `.ecky` version source without a
  verify clause
- WHEN the author inserts a verify template and applies the draft
- THEN render uses source that includes the inserted top-level verify clause
- AND no history version is committed implicitly.

#### Scenario: Version-mode commit keeps inserted verify source

- GIVEN the workbench code inspector opens real `.ecky` version source without a
  verify clause
- WHEN the author inserts a verify template and commits a new version
- THEN the committed version stores source that includes the inserted top-level
  verify clause
- AND render uses that same verify-bearing source during commit validation.

#### Scenario: Version-mode Ecky source is highlighted as Ecky

- GIVEN the workbench code inspector opens real `.ecky` source
- WHEN the modal renders the editor
- THEN Ecky comments, keywords, numbers, strings, and atoms are highlighted as
  Ecky tokens
- AND the editor does not present that source as Python mode.

#### Scenario: Docs-opened Ecky snippet shows visible Ecky colors

- GIVEN the docs window opens an Ecky tutorial snippet in the code inspector
- WHEN the scratch modal renders
- THEN Ecky keyword and number tokens use the shipped tactical highlight colors
- AND the docs scratch modal remains copy-only.

