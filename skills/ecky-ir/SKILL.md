---
name: ecky-ir
description: Read and author Ecky IR source. Use when working with .ecky models, params, components, selectors, exact CAD ops, or verify clauses.
---

# Ecky IR

Use this skill for `.ecky` source work: write, review, refactor, or explain
Ecky models.

Read first:

- `public/docs/ecky-ir.md`
- `docs/books/ecky-ir/index.md`
- `docs/generated/ecky-agent-system-prompt.md`

## Core rules

- Output one complete `(model ...)` program.
- `params` are user-visible controls and model signature. Do not replace them
  with `let`.
- Use `let` / `let*` for local derived values only. Use `let*` when later
  bindings depend on earlier ones.
- Keep part ids, feature ids, selector ids, and binding names stable.
- Prefer `repeat`, array forms, or component instances for repeated geometry.
- Name fit-critical dimensions and relations. Do not bury them in anonymous
  offsets.
- Keep `verify` clauses measurable. Fix geometry or parameters when a clause
  fails; do not weaken the check to make it pass.

## Language workflow

1. Identify the user-visible signature first: `params`, `feature :params`, or
   component signature entries.
2. Separate signature values from internal math. Signature values belong in
   `params`; derived values belong in `let*`.
3. Check op signatures in the reference before inventing syntax.
4. Respect backend support. If the active backend rejects an op, change the op
   or backend.
5. Prefer stable semantic bindings over topology indices.

## Good shape

```scheme
(model
  (params
    (number width 60 :label "Width" :min 20 :max 120)
    (number height 40 :label "Height" :min 10 :max 80))
  (part body
    (let* ((radius (/ width 2))
           (hole-r 3))
      (difference
        (cylinder radius height)
        (translate 0 0 -0.5 (cylinder hole-r (+ height 1)))))))
```

## Bad shape

- `let` used where the user should see a knob.
- hard-coded fit dimensions that should be adjustable.
- anonymous repeated geometry blocks copied by hand.
- raw topology ids treated as stable authoring intent.

