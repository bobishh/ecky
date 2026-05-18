# Design: Verify Authoring Foundation

## Goal

Give `.ecky` source one explicit place to declare authored verification metadata
and ship one additive runtime evaluation path without creating a new result
contract.

## Decisions

- `verify` is top-level only in this slice.
- Each verify clause has exactly three named sections: `tag`, `metric`,
  `expect`.
- Section payload is preserved as authored data in compiler/core IR.
- Core IR stores verify clauses under `CoreProgramConstraints`.
- Shared structural verification helpers evaluate authored verify clauses
  additively when runtime bundle source is Ecky and source file is readable.
- Failed or errored authored checks map onto existing `StructuralIssue`
  records and recompute `passed`/`summary` on the returned structural result.
- File-backed docs route exposes verify authoring syntax and opens a snippet into
  the existing code inspector.
- Existing code inspector exposes one-click verify template insertion for
  model-shaped source and disables the action when verify already exists.
- Docs-opened snippets use a scratch-only code-inspector mode with no apply,
  commit, or fork actions.
- Docs-opened snippets stay copy-only: no verify insertion action and no
  source-mode badge chrome.
- Version-mode workbench editing remains the only path that can apply or commit
  inserted verify source into live history.
- `.ecky` code uses a local stream tokenizer plus fixed token classes instead of
  the Python language mode.

## Rejected Paths

- Embedding verification inside geometry operators. Rejected because it mixes
  CAD authoring with verification metadata surface.
- Database lookup from sync verify handlers. Rejected because runtime bundle
  already carries `macro_path`, and sync handler DB locking would couple MCP
  verification to async runtime constraints.
- Normalizing metric names or expectation forms into a new public contract now.
  Rejected because existing `StructuralVerificationResult` is enough for first
  additive reporting.
- Direct insertion from docs window into live working-copy source. Rejected
  because current inspector contract is whole-buffer edit, and scratch-modal
  seeding is lower-risk.
- Reusing live version-editing actions for docs snippets. Rejected because docs
  browsing would gain accidental overwrite path into active workbench state.
- Reusing version-only verify insertion affordances in docs snippets. Rejected
  because docs code is explanatory scratch text, not live authoring state.
- Leaving `.ecky` in Python mode. Rejected because syntax colors and token
  boundaries lie about authoring language.
- Adding transform or repair semantics now. Rejected because parser foundation
  and additive evaluation must stabilize first.

## Syntax

Starter syntax:

```clojure
(verify
  (tag front_entrance body.front_window_1)
  (metric front_overlap (projection-overlap lid.front_skirt body.front_window_1 :axis x))
  (expect front_overlap (> 3)))
```

Grammar shape:

```text
top-level-form := part-form | feature-form | params-form | verify-form
verify-form    := (verify tag-section metric-section expect-section)
tag-section    := (tag verify-value*)
metric-section := (metric verify-value*)
expect-section := (expect verify-value*)
verify-value   := symbol | string | boolean | number | list
list           := (verify-value*)
```

Constraints:

- `verify` nested under geometry or expression forms rejects.
- empty or malformed `(verify ...)` rejects.
- section names must appear in `tag` then `metric` then `expect` order.
- section payload may be empty, but section itself must exist.

## Core Model

Core IR adds verify storage in existing program constraints:

```text
CoreProgramConstraints
  relations: Vec<CoreRelationConstraint>
  verify_clauses: Vec<CoreVerifyClause>

CoreVerifyClause
  tag: CoreVerifySection
  metric: CoreVerifySection
  expect: CoreVerifySection

CoreVerifySection
  items: Vec<CoreVerifyValue>

CoreVerifyValue
  Symbol(String)
  Number(f64)
  Boolean(bool)
  Text(String)
  List(Vec<CoreVerifyValue>)
```

Preservation rules:

- preserve top-level verify clause order
- preserve section order and item order
- preserve nested list shape
- preserve payload across direct parser path, runtime parser path, and emitted
  legacy source reparse

No source-span capture, check ids, or metric enums in this slice.

## Execution Order

Implemented pipeline:

```text
.ecky source
  -> parse top-level verify clause
  -> store verify payload in CoreProgramConstraints
  -> emit legacy source from CoreProgram
  -> reparse emitted source without verify payload loss
  -> runtime bundle carries Ecky source path
  -> structural verification computes base StructuralVerificationResult
  -> authored verify evaluator reads source, recompiles, resolves metrics
  -> authored fail/error checks append StructuralIssue entries
  -> command or MCP response returns merged structural result or summary
```

Runtime contract in this slice:

- render path still ignores authored verify clause semantics
- structural verification entrypoints that already load runtime bundles evaluate
  authored clauses additively
- existing structural verifier remains base metric producer and result carrier
- docs route and docs window expose verify authoring examples; inline editor
  widgets remain unchanged
- code inspector can append one starter verify clause into model-shaped source
  and reports duplicate state without mutating existing verify clauses
- docs-opened snippets stay local scratch buffers and cannot save or render into
  the active version
- docs-opened snippets expose copy-only action surface and do not show
  source-mode badges or verify insertion controls
- version-mode workbench editing can render or commit the inserted verify source
  through existing manual authoring commands
- `.ecky` source gets keyword/comment/string/number/atom token highlighting with
  fixed classes and forced tactical colors that browser proof can target

## Current Metric Surface

Current evaluator supports a bounded metric set:

- manifest: `has-step`, `has-preview-stl`, `edge-target-count`,
  `face-target-count`, `export-format-count`, `part-count`
- stl: `triangle-count`, `connected-component-count`,
  `non-manifold-edge-count`, `overhang-face-count`

## Next Phase

Follow-up design should define:

- whether to formalize current metric names as stable external contract
- richer comparator or boolean composition grammar if needed
- whether authored results need dedicated contract fields instead of
  `StructuralIssue` projection
- whether UI/editor surfaces should expose authored verification state directly

Existing author-verification metric rollups now power MCP merge. Broader public
surface remains follow-up work.

## Phase Boundaries

This change ends after:

- parser accepts and rejects top-level verify syntax correctly
- core IR preserves verify clause payload
- legacy emit reproduces verify clause syntax
- roundtrip proof shows tag/metric/expect payload survives reparse
- structural verification entrypoints evaluate authored clauses and report
  additive fail/error issues

This change does not include:

- transform recipes
- UI authoring widgets
- dedicated authored verification contract fields

## Proof Plan

- parser tests for valid top-level verify plus invalid nested and empty verify
- runtime parser parity test for verify payload preservation
- emit test for verify clause serialization
- roundtrip test for verify payload preservation after emit and reparse
- shared helper tests for authored merge and compile-error fallback
- MCP test for authored verify failure merging into structural verification
- MCP test for authored verify error mapping
- MCP summary test for additive authored failure reporting
- docs route and docs-window tests for verify authoring snippet visibility
- code-inspector browser test for verify insert happy path and duplicate state
- docs-window browser test for copy-only scratch snippet mode
- workbench code-path tests for inserted verify apply and commit persistence
- Ecky tokenizer unit test plus docs/workbench browser proof for highlighted
  tokens
