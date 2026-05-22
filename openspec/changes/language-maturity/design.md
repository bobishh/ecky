# Design: Language Maturity

## Architecture stance

Same move as component-unification everywhere it is possible: a powerful
authored surface lowered into the existing finite Core IR before geometry
runs. Units, tags, and views are surface + verifier features; Core IR grows
only where a concept must survive into planning (tag anchors, view layers).

## G2: Units

```scheme
(params (number plate_w 70mm :min 40mm :max 120mm)
        (number rotate_z 45deg))
(part plate (extrude (rounded-rect plate_w 42mm 5mm) 4mm))
```

- Lexing: `<numeric><suffix>` where suffix in `mm|cm|m|in|deg|rad`. Lowered in
  the shared source pre-pass (same hook as `define-component` lowering) into
  canonical numbers: lengths -> mm, angles -> deg. `1in` -> `25.4`,
  `0.5rad` -> `28.6478897565deg`.
- The pre-pass records each literal's dimension (length|angle|scalar) keyed by
  span into a side table handed to the verifier; params keep dimension from
  suffix or `:unit`.
- Dimension table for ops (data, not code): per CoreOperation argument slots
  declare expected dimension (`box: [len, len, len]`, `rotate: [angle x3,
  solid]`, counts/segments: scalar). Verifier walks calls; mismatch = error
  with span: "`rotate` expects an angle, `plate_w` is a length (70mm)".
- Strictness: `(meta units strict)` opts a model in; default is permissive
  (suffixes normalize, mismatches warn in diagnostics) until the book and
  generation prompts migrate, then default flips. Both modes share one code
  path; strictness only raises severity.
- Emit preserves suffixes? No: emit prints canonical numbers; the params panel
  renders the dimension as the unit chip it already supports via `:unit`.
  (Roundtrip fidelity of suffixes is a non-goal; provenance is the version.)

## G3: Stable selectors

```scheme
(part bracket
  (build
    (shape blank (extrude (rounded-rect 70 42 5) 6))
    (shape pocket (translate 20 0 2 (box 18 12 6)))
    (shape body (difference blank pocket))
    (result
      (fillet 1.5 :edges (tag-edges pocket_rim :created-by pocket) body))))

(tag-face mounting_top :face "top" bracket_body)
```

- `tag-face`/`tag-edges` create named anchors: at plan time the inner selector
  resolves exactly as today, and the resulting stable/durable target ids are
  recorded under the tag name in the manifest (`taggedAnchors`).
- Any place a selector string is accepted also accepts `(tag <name>)`;
  resolution prefers the recorded ids and falls back to re-resolving the
  authored selector when ids no longer match (re-binding with a diagnostic).
- `:created-by <shape>` filters candidate faces/edges to topology produced by
  that `build` binding. Implementation route: the planner already knows which
  OcctSlot produced which command; the runner's topology report gains
  per-face/edge originating-slot indices (C++ change, additive field). This is
  the L-sized part and lands last, behind its own task.

## G4: Views and assemblies

```scheme
(view exploded
  (offset-part top_half 0 0 40)
  (offset-part film_carrier 60 0 0))
```

- `view` is a model clause: named sets of per-part preview transforms. Lowered
  into manifest view entries (names + per-part offsets); the viewer applies
  them as display transforms. Export pipelines never read views — enforced by
  the same gate test pattern as debug overlays (G-VIEW gate: artifact digests
  identical with and without views).
- `assembly`/`export` clauses: spec'd grammar reserved now, implementation
  deferred until views prove the display/manufacturing split (they formalize
  what component packages already do at the package layer).

## G5: Diagnostics

- One enrichment type threaded through normalize -> plan -> export:
  `{ part_key, op_name, span, resolved_params: [(key, value)] }`.
- Failure sites attach it; `AppError.details` gains a structured tail the UI
  and agents can parse: `part=female_rail op=difference rail_tip_w=8 rail_h=2`.
- Verify failures already carry tags/metrics; they adopt the same parameter
  echo so "clearance below expected 0.3mm" includes the values that produced
  the measurement.

## G6: Verify-TDD loop

Authoring contract (prompt + card + book): a generated model MUST ship with
`(verify ...)` clauses for its load-bearing claims (fit clearances, wall
minimums, manifoldness, step export) — written from the *requirements*, before
geometry is trusted. First render is expected red.

Loop (existing pieces wired, not new machinery):

```text
generate (model + verify) -> render -> verify_generated_model
  green -> present, chips green
  red   -> format failures (tag, metric, expected vs actual, involved params)
        -> append as feedback turn -> regenerate (<= max_verify_attempts)
  still red -> present honestly with red chips + failure table
```

- `max_verify_attempts` default moves 0 -> 2.
- Feedback formatter is deterministic text, e.g.
  `verify hinge_a/clearance_check: clearance min-distance = 0.12, expected >= 0.3 (pin_d=8, bore=8.3)`.
- UI: per-tag red/green chips on the version card; clicking focuses the verify
  node in the New Params map (the map already renders verify-capable nodes).
- Guides: authoring card gains a "verify-first" rule; the book's verify
  chapter gains the TDD framing with one worked red->green example.

## Compatibility gates

- G-KEY/G-EMIT/G-CORE/G-RENDER from component-unification re-run unchanged.
- G-VIEW: views never alter export artifact digests.
- G-UNIT: all existing fixtures compile identically in permissive mode.

## Risks

- Dimension table drift vs. op reality — table lives next to
  `verify_core_program` signatures and is covered by a completeness test
  (every CoreOperation has an entry or an explicit scalar-only marker).
- Runner ABI change for `:created-by` — additive JSON field, version-gated by
  the existing runner exit-code contract.
- Verify-TDD cost: each retry is a render; capped by attempts and only runs
  when the model authored verify clauses.
