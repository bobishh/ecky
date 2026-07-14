# Design: Define Binding Scope

## Why not fix `define` mechanics?

Three options were considered:

### Option 1: Rule (chosen)

Reject `define` inside `model` with a clear error. Zero architecture change.
Matches the existing design: `let*` is the primitive for computed parametric
values; `define` is for top-level helper functions.

### Option 2: Evaluate at define time (rejected)

Inject param values into Steel before evaluating the `define`. **Fatal flaw**:
breaks parametricity. The model would be evaluated once with default values
and couldn't respond to parameter slider changes. The entire system works
because params survive as symbolic references through to Core IR.

### Option 3: Lazy define (rejected)

Intercept `define` before Steel and convert to a deferred form. **What this
really is**: reimplementing `let*` with a different spelling. `let*` already
does exactly this — it's a binding whose body is evaluated lazily within the
part scope. Inventing a second mechanism doubles the binding surface for no
user benefit.

## Where the guard fires

```
compile_to_core_program(source)
  → reject_model_level_sequence_forms(source)     ← guard fires HERE
  → lower_component_definitions_source(source)
  → can_use_expanded_ast? → expanded AST path     ← define never reaches here
                          → runtime path           ← define never reaches here
```

`reject_model_level_sequence_forms` runs on both the legacy and strict-units
paths (lines 175 and 190), before any Steel evaluation. The guard catches
`define` inside `model` in the `reject_model_level_sequence_form_group` match
arm, alongside the existing `map`/`range` rejection.

## Architecture context

- **Steel** (external crate `steel-core` 0.8.2): parsing, AST expansion,
  eager runtime evaluation.
- **Ecky Core IR** (3.5K lines in `ecky_core_ir/`): Ecky's own IR with
  parametric evaluation, type checking, constraint validation.
- **Ecky compiler** (12.5K lines in `compiler.rs`): source transformation
  layer that rewrites `.ecky` source before Steel and decodes Steel's output
  into Core IR.

The guard lives in Ecky's source transformation layer — no Steel internals
touched.
