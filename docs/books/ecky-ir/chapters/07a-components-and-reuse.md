## Components and Reuse: Lift a Proven Part

`repeat` solves "the same shape, many times, in one part." It does not solve "the same _proven_ shape, in two different parts, with its checks coming along." The moment you copy a block of geometry from one part into another, you have made a second thing to maintain — and the day you change the wall thickness in one and forget the other is the day a print fails. A **component** is the fix: name the geometry once, reuse it by reference, and let its proof travel with it.

Say you have dialed in a mounting standoff — a bored post whose wall must stay thick enough to survive a screw. Lift it into a `define-component`:

```scheme
(define-component standoff
  ((number height 12 :label "Standoff height" :min 6 :max 30)
   (number bore 3.2))
  (verify (tag bore_open) (metric min_wall_thickness "body") (expect (>= value 1.2)))
  (difference
    (cylinder 6 height 96)
    (cylinder bore (+ height 2) 96)))

(model
  (part front_left (standoff :height 16))
  (part rear_right (translate 40 0 0 (standoff))))
```

Three ideas earn their keep here.

**Reuse by reference, override by keyword.** `(standoff :height 16)` instantiates the component and overrides one signature key; `(standoff)` takes every default. Omitted keys fall back to the signature, and a missing _required_ key (one with no default) is a compile error that names the component and lists its signature. There is no copy-paste, so there is no drift: change the body once and both parts move together.

**Closedness is the whole contract.** A component body sees only its signature keys plus bindings it makes itself (`let`, `let*`, `repeat` indices, `build` shapes). It cannot reach a model param or an outer `let*` — try it and you get a compile error naming the variable. That restriction is not a nuisance; it is what makes a component _copy-inlineable_. Paste the `define-component` into any other model and it just works, because it never depended on its surroundings.

**Proof travels with the part.** The `verify` clause lives inside the component, so it expands once per instantiation, its tag namespaced by the part key — `front_left/bore_open`, `rear_right/bore_open`. Reuse therefore includes the wall-thickness check at every call site for free. You proved the part once; every future use re-proves itself.

For the exact signature grammar, nesting limits, and verify-travel rules, see **`define-component`** in the language reference appendix.

### The library loop (MCP)

Components do not have to live in one file. Agents lift proven parts into a shared library and pull them back by source:

1. `component_extract` — hand it a model and a `partKey`. Referenced model params become the signature (metadata preserved); scalar outer bindings become plain defaults; any non-scalar free reference is reported as a blocker so you cannot extract something that secretly depends on its context. `save: true` stores it.
2. `component_search` — compact headers only (name, one-liner, param keys, tags). Bodies never come back from search, so the library stays browsable.
3. `component_get` — the full, self-contained `define-component` source for one name. Paste it into the model and instantiate.

The loop is copy-inline by design: what you get back is closed source, not a hidden registry link. A part proven in one project becomes a building block in the next, checks and all.
