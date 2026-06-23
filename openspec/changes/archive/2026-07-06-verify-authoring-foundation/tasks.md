# Tasks: Verify Authoring Foundation

## 1. Shipped Slice: Parser and Roundtrip

- [x] 1.1 Add compile test for valid top-level `(verify ...)` beside model
  geometry.
- [x] 1.2 Add compile tests for nested `(verify ...)` rejection and empty
  `(verify)` rejection.
- [x] 1.3 Implement top-level `verify` parsing with required `(tag ...)`,
  `(metric ...)`, and `(expect ...)` sections.
- [x] 1.4 Preserve verify payload as symbols, strings, booleans, numbers, and
  nested lists in core IR.
- [x] 1.5 Emit verify clauses back to legacy source syntax.
- [x] 1.6 Prove verify payload roundtrips through parse -> emit -> parse.
- [x] 1.7 Prove runtime parser path preserves verify payload too.

## 2. Current Boundaries

- [x] 2.1 Keep authored verify clauses additive metadata only.
- [x] 2.2 Reuse existing `StructuralVerificationResult` and summary responses
  for additive reporting.
- [x] 2.3 Keep UI/editor verification surfaces unchanged in this slice.

## 3. Current Phase: Manifest And Structural Evaluation

- [x] 3.1 Define current manifest-backed authored metrics and argument shapes.
- [x] 3.2 Define current structural-backed authored metrics and argument
  shapes.
- [x] 3.3 Evaluate authored `metric` and `expect` payload against runtime
  manifest and structural evidence in shared structural verification helpers.
- [x] 3.4 Surface per-clause pass/fail/error results through additive raw
  structural issues.
- [x] 3.5 Attach authored verification results beside existing structural
  verification output, not as replacement.
- [x] 3.6 Keep regression coverage for missing/unsupported evidence handling and
  additive reporting.
- [x] 3.7 Reuse shared authored merge helper from Tauri and MCP verification
  entrypoints.
- [x] 3.8 Keep authored verify failures explicit in retry and terminal
  structural failure copy.

## 4. Later Expansion

- [ ] 4.1 Add boolean composition or richer expectation grammar if runtime
  evaluation needs it. — deferred: not demanded by current runtime; revisit in a future change
- [ ] 4.2 Add transform or repair clauses only after evaluation semantics are
  stable. — deferred: not demanded by current runtime; revisit in a future change
- [x] 4.3 Add docs-based UI/editor authoring surface after runtime result
  plumbing exists.
- [x] 4.4 Add lightweight code-inspector verify template insertion with
  duplicate guard and browser proof.
- [x] 4.5 Keep docs snippet code-inspector mode scratch-only so it cannot apply,
  fork, or commit into live authoring state.
- [x] 4.6 Prove version-mode workbench code can apply and commit inserted verify
  template through the existing manual authoring flow.
- [x] 4.7 Replace fake Python mode on `.ecky` code with real Ecky token
  highlighting and proof on real workbench modal.
- [x] 4.8 Strip docs-snippet-only badge and verify controls from `OPEN IN CODE`
  docs scratch modal, and harden visible Ecky token colors with browser proof.
