# Tasks: Language Maturity

## Worker Rules

- Additive only: existing fixtures keep byte-identical stable node keys,
  emit spellings, and render digests (re-run component-unification locks).
- Core IR public structs unchanged unless a task explicitly says otherwise.
- TDD per slice; `cd src-tauri && cargo check` before any success claim.
- No commits/staging.

## 1. G1 - Reuse (S)

- [x] 1.1 Book chapter: hoisting derived values with model-level `let*`,
  helper `define`, and `define-component`; one worked de-duplication of a
  repeated-parts model.
- [x] 1.2 Authoring card rule: derived values appear once; repetition across
  parts means a missing binding or component.
- [x] 1.3 Fix any compiler gaps the chapter exposes (tracked as found).

## 2. G2 - Units (M core, L strict)

- [x] 2.1a Pre-pass: unit literal suffixes (mm|cm|m|in|deg|rad) normalize to
  canonical mm/deg.
- [x] 2.1b Dimension side-table by span for verifier diagnostics.
- [x] 2.2 Param dimension from suffix or `:unit`; params panel shows the chip.
- [x] 2.3 Op dimension table + completeness test (every CoreOperation listed).
- [x] 2.4 Verifier dimension check; permissive mode warns, `(meta units
  strict)` errors with span and both dimensions named.
- [x] 2.5 G-UNIT lock: existing fixtures identical in permissive mode.
- [x] 2.6 Book + authoring card: units section; generation prompt emits
  suffixed literals.

## 3. G3 - Selectors (M tags, L provenance)

- [x] 3.1a `tag-face`/`tag-edges` grammar accepted on expanded and runtime
  compile paths; authored selector declarations retained in CoreProgram.
- [x] 3.1b Plan-time resolution -> manifest `taggedAnchors` (name ->
  stable/durable ids + authored selector).
- [x] 3.2a `(tag <name>)` accepted in selector keyword payloads as a tagged
  target placeholder.
- [x] 3.2b Tagged selectors prefer recorded ids, re-resolve + diagnostics on
  mismatch.
- [x] 3.3 Parameter-sweep test: fillet on a tagged face survives a dimension
  change that re-indexes faces.
- [x] 3.4 Runner topology report: originating-slot index per face/edge
  (additive ABI field, C++ + Rust reader).
- [x] 3.5 `:created-by <shape>` selector filtering on provenance.
- [x] 3.6 Book + card: when to tag (any fit-critical selector).

## 4. G4 - Views (M)

- [x] 4.1 `(view name (offset-part part dx dy dz) ...)` clause -> manifest
  view entries; viewer applies as display transforms with a view switcher.
- [x] 4.2 G-VIEW gate test: export artifact digests identical with/without
  views (debug-overlay pattern).
- [x] 4.3 Reserve `assembly`/`export` grammar in the book as "planned",
  implementation deferred.

## 5. G5 - Diagnostics (M)

- [x] 5.1 Enrichment struct (part, op, span, resolved params) attached at
  normalize/plan failures.
- [x] 5.2 Native export/runner failures map to the same shape.
- [x] 5.3 Verify failures echo involved parameter values.
- [x] 5.4 UI surfaces the structured tail at the responsible node (map +
  status); agents get it verbatim in MCP errors.

## 6. G6 - Verify-TDD (REALITY: loop already exists; remaining is one seam) (S)

Existing (verified in code, do NOT rebuild):
- Two-stage loop `src/lib/controllers/verificationLoop.ts` (structural -> visual
  screenshot, repair prompts, attempt budgets) + tests.
- `author_verification_foundation.rs` (2034 LOC): authored `(verify ...)`
  clauses evaluated per tag with structured `expected`/`actual`/`metric_key`.
- MCP `verify_generated_model` returns `StructuralVerificationResult` with
  `authoredVerifyChecks: [{ tag, status, message, stableNodeId }]`.
- Authoring card already mandates "model + verify together".

The MCP-first TDD loop is therefore already functional: the agent renders,
calls `verify_generated_model`, reads per-tag red/green with the delta in
`message` ("0.12 >= 0.3"), fixes, re-renders. The agent owns the loop.

Remaining gap: the delta is a STRING. The structured fields
(`metric_source`, `metric_key`, comparator, `expected`, `actual`) exist in the
internal `AuthorVerifyCheckResult` but are dropped in the public
`AuthoredVerifyCheck` (and `stableNodeId` is hardcoded `None`). So agents and
UI chips parse text instead of reading machine values.

- [x] 6.1 Public `AuthoredVerifyCheck` gains machine-readable delta:
  `metricSource`, `metricKey`, `comparator`, `expected`, `actual`
  (camelCase boundary; `AuthoredVerifyValue` = number|boolean|text). Thread the
  comparator through `AuthorVerifyCheckResult` (only the value is stored today).
- [x] 6.2 Map the new fields in `authored_verify_check_contract`; populate
  `stableNodeId` from the clause where available instead of `None`.
- [x] 6.3 Regenerate `contracts.ts`; assert the MCP `verify_generated_model`
  response carries the structured delta (extend the existing
  `verify_generated_model_surfaces_authored_verify_errors` test).
- [x] 6.4 UI: `versionAuthoredVerifyCards` renders red/green per tag from the
  structured fields (expected vs actual), click focuses the verify node in the
  New Params map. (Confirm what already renders before adding.)
- [x] 6.5 Book verify chapter: explicit MCP TDD framing — author verify from
  requirements, expect red first, `verify_generated_model` -> fix -> re-render;
  one worked red->green example.

## Proof Gates

- [x] G-LOCKS component-unification key/emit/core/render locks stay green.
- [x] G-UNIT permissive mode leaves every existing fixture untouched.
- [x] G-VIEW views never change export digests.
- [x] G-TDD a seeded red-verify model goes green within the retry cap in e2e.

## Suggested order

G6 first (highest leverage per effort: prompts + existing loop), then G2
suffixes (2.1-2.2), G3 tags (3.1-3.3), G5, G4, then the L-tails (2.3-2.4
strict units, 3.4-3.5 provenance).
