# Proposal: Verify Authoring Foundation

## Intent

Add first-class authored verification metadata to `.ecky` source and ship first
additive runtime evaluation path for structural verification surfaces that
already consume runtime bundles.

Today source cannot carry explicit verification intent through compile and emit
paths. Authors can write geometry, params, and relations, but cannot persist a
top-level verify clause that runtime verification can inspect and report.

This change keeps authored `tag`/`metric`/`expect` payload alive through parse,
core IR, legacy emit, and reparse, then evaluates current manifest/STL-backed
checks through shared structural verification helpers without introducing a new
public result contract.

## Scope

- Add parser support for top-level `(verify ...)` clauses under `(model ...)`.
- Require each verify clause to contain `(tag ...)`, `(metric ...)`, and
  `(expect ...)` sections.
- Preserve section payload as authored symbols, strings, booleans, numbers, and
  nested lists.
- Store verify clauses in core program constraints.
- Emit verify clauses back to legacy source form.
- Prove roundtrip through compile and emit paths.
- Evaluate current authored verify clauses against runtime manifest and
  structural evidence in shared structural verification helpers.
- Surface authored pass/fail/error additively through existing
  `StructuralVerificationResult` and summary responses.
- Add file-backed docs coverage for verify authoring and snippet-open workflow.
- Add lightweight code-inspector verify template insertion for model-shaped
  source.
- Keep docs-opened snippets scratch-only so docs browsing cannot mutate the live
  workbench version.
- Keep docs-opened snippets copy-only with no source-mode badge or verify
  insertion controls.
- Prove inserted verify template survives version-mode apply and commit flows in
  the real workbench code path.
- Replace fake Python highlighting on `.ecky` source with local Ecky syntax
  highlighting in the code inspector.

## Out of Scope

- Dedicated inline editor widgets or bespoke verify form builders.
- Rich authored result contract with durable per-check ids or source spans.
- Scenario transforms, repair suggestions, or mutation recipes.
- Non-top-level or operation-local `verify` clauses.

## First Implemented Slice

Starter authored surface:

```clojure
(verify
  (tag body_shell)
  (metric check (manifest has-step))
  (expect check (= true)))
```

Payload stays opaque in compiler/core IR:

- `tag` carries authored labels and references.
- `metric` carries authored metric name plus arguments.
- `expect` carries authored expectation form.

Runtime evaluation now exists in current structural verification entrypoints that
already load runtime bundles:

- compile preserved verify clauses from readable Ecky source bundle
- evaluate current manifest and STL-backed metrics
- map authored fail/error checks into additive structural issues
- preserve existing structural verifier as base authority and result carrier

## Approach

Phase work in three steps:

1. parser/core IR support for top-level verify clauses
2. legacy emit plus reparse proof for authored payload preservation
3. shared structural verification merge using existing structural result carrier

Reject nested or malformed verify clauses now so follow-up runtime work starts
from stable source shape instead of loose syntax.

## Next Phase

Next change should extend current additive runtime work:

- widen metric registry beyond current manifest/STL checks
- define durable per-check ids or source anchors if downstream tooling needs them
- add dedicated authored verification UI/editor affordances if users need them

Current structural verification responses remain additive. Narrow docs-based
authoring help now exists; code inspector can seed one verify template in real
version editing; bespoke editor widgets still remain unchanged. Docs snippet
mode stays copy-only scratch. Workbench version-edit flow now has explicit
apply/commit proof for inserted verify source. Code inspector now distinguishes
`.ecky` tokens instead of rendering them as Python, and docs snippet colors are
visibly enforced.
