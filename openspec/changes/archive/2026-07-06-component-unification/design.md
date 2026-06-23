# Design: Component Unification

## Architecture

```text
Authored surface (.ecky)
  model | part | define-component | instantiation   <- spellings
            |
            v
  AST: one node kind `component` { role, spelling, signature, body }
            |
            v  compile-time inline expansion (lexical params, defaults, overrides)
            |
  CoreProgram { parameters, parts, constraints }    <- UNCHANGED
            |
            v
  normalize -> OcctPlan -> runner/generated source -> STEP/STL/topology
```

The expansion step is the same architectural move the normalizer already makes
for `repeat`/`map`/`apply`: a powerful authored surface lowered into the finite,
dumb IR before any geometry code runs. Nothing downstream of
`compile_to_core_program` changes.

## Entity model

One node kind, three roles:

| Role | Surface spelling | Today's equivalent |
| --- | --- | --- |
| `root` | `(model ...)` | the model |
| `output` | `(part id [label] expr)` | a part (topology boundary) |
| `library` | `(define-component name (sig...) body)` | new |

- A `root` component's `(params ...)` block is its signature. Identical
  semantics to today's global model params.
- An `output` component is anonymous, zero-arg, instantiated exactly once at
  its declaration site, capturing the enclosing lexical scope — which is
  exactly what `(part ...)` already is. Output components remain the only
  topology part boundaries; arbitrary recursion does not create new
  topology parts.
- A `library` component is never rendered directly. It produces geometry only
  through instantiation.

`feature` keeps its current parse path; it is already a role-tagged part and
folds into the same node kind with its existing role string.

## Spelling preservation

`spelling` is a stored attribute on the AST node, never derived. Emit writes
the original spelling back. `model`/`part` sources never re-emit as
`component`. This keeps AST patches diff-minimal and protects existing
emit/roundtrip tests.

## Signature and scoping

```clojure
(define-component knuckle
  ((number pin_d 8 :label "Pin diameter" :min 4 :max 12 :step 0.5)
   (number clearance 0.3))
  (difference
    (cylinder (* 2 pin_d) 10 96)
    (cylinder (+ pin_d clearance) 12 96)))

(part hinge_a (knuckle :pin_d 6))
(part hinge_b (knuckle))            ; defaults apply
```

- Signature entries reuse the existing `(params ...)` entry grammar
  (kind, key, default, keyword metadata). One grammar, two positions.
- Instantiation is `(name :key value ...)`. Unknown keyword -> deterministic
  compile error naming the component and its signature. Missing required
  (no-default) entry -> deterministic compile error.
- Scoping is lexical. The body sees its signature bindings plus nothing else
  from the call site. Free variables in a `define-component` body that are not
  in the signature are a compile error (this is what makes components closed
  and therefore copy-inlineable).
- On the Steel runtime compile path, `define-component` lowers to a plain
  `define` + lambda with keyword arguments; the structural expanded-AST path
  implements the same semantics through the existing `ExpandedHelperMap`
  mechanism. Both paths must produce identical CorePrograms (parity test).

## Expansion and provenance

Instantiation inline-expands the body with signature bindings substituted,
allocating fresh node ids (same id-allocation discipline as `repeat`
expansion). The call-site source node is recorded as the source anchor for the
expansion, so source-addressability degrades no further than today's
`repeat`: clicking expanded geometry maps back to the call site.

Stable node keys for `model`/`part` spellings must remain byte-identical to
current derivation. This is a hard compatibility gate (existing threads store
selectors, AST patch targets, and edit-cycle topology aliases derived from
them). New spellings get new keys; old spellings keep old keys.

## Verify travel

`verify` clauses inside a `define-component` body expand once per
instantiation, with `(tag ...)` namespaced by the instantiating part key
(`hinge_a/clearance_check`). Authored top-level verify behavior is unchanged.
A pasted component therefore carries its own checks — reuse includes proof.

## Extraction (library direction)

`component_extract` (MCP) lifts an existing part subtree into a
`define-component`:

1. Resolve target part by stable node key or part key.
2. Free-variable analysis over the subtree: referenced model params and outer
   `let*` bindings become the signature (params keep their metadata; bindings
   become plain defaults from their current evaluated values when scalar,
   otherwise extraction reports them as blockers).
3. Output: component `.ecky` source + header JSON
   `{ name, params: [...], tags: [...], provenance: { threadId, messageId,
   sourceDigest }, interfaces: [named constraint keys referenced] }`.
4. Copy-inline only: the returned source is self-contained and pasteable.
   No registry reference is created implicitly.

Library storage reuses `component_package_runtime` conventions: one directory
per component under the existing component-library dir, containing
`component.ecky` + `ecky-header.json`. `component_search` (MCP) scans headers
only and returns compact results (name, one-liner, param keys, tags) so agents
never pay for bodies during search.

## Compatibility gates

- G-KEY: stable node keys for every existing fixture parse byte-identical
  before/after this change.
- G-EMIT: every existing fixture roundtrips with original spellings.
- G-CORE: `CoreProgram` structs and `verify_core_program` are untouched.
- G-RENDER: existing fixture renders produce identical artifact digests.

## Risks

- Steel runtime path and expanded-AST path drifting: mitigated by mandatory
  parity tests per feature (same source -> same CoreProgram on both paths).
- Free-variable analysis under `let*` shadowing: extraction must use the
  compiler's actual binding resolution, not a regex; tasks point workers at
  the existing helper-map machinery.
- Key derivation accidentally keyed off node kind names: T3 lands the
  fixture-lock test FIRST, before T1/T2 merge, so any drift fails loudly.
