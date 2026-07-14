# Proposal: Define Binding Scope

## Intent

Make `(define ...)` inside `(model ...)` fail immediately with a clear,
actionable error instead of falling through to Steel's eager evaluator and
producing a misleading `TypeMismatch` on the first arithmetic operation.

## Problem

`(define x (- frame_length 2))` placed inside `(model ...)` is evaluated by
Steel before params have concrete values. Steel sees the param as a bare
symbol and its arithmetic ops reject it, yielding:

    TypeMismatch: - expects a number, found: 'frame_length

This error names neither the real cause (`define` inside `model`) nor the
fix (`let*` inside the part). Users — including LLM agents — hit this
repeatedly because `define` is a natural way to name a derived value.

## Root Cause

- `seed_symbol_bindings` (compiler.rs) registers every param as
  `SteelVal::SymbolV(name)` — a symbolic placeholder, not a number.
- `define` is eagerly evaluated by Steel's `compile_and_run_raw_program`
  before Ecky's Core IR layer can intervene.
- `let*` works inside `(model ...)` because the model body is preserved as
  unevaluated data via `rewrite_runtime_model_clause_group_source`, so the
  symbolic bindings survive to Core IR evaluation where params are resolved.

## Scope

- Intercept `(define ...)` in `reject_model_level_sequence_form_group`
  (the early validation pass that runs before any Steel evaluation).
- The new error names the cause, the allowed top-level form, and the fix
  (`let*` inside `part`), with a copy-pasteable example.
- System prompt updated so the LLM never emits `define` inside `model`.
- Docs updated with a "Common mistake" callout in the repetition chapter.

## Out of Scope

- Changing `define` mechanics (lazy evaluation / param substitution).
  Considered and rejected: lazy define would reimplement `let*` under a
  different name, doubling the binding surface for no user benefit.
- Detecting top-level value defines that reference params (e.g.
  `(define half (/ frame_length 2))` outside model). These currently
  compile but the param reference resolves to a symbol at use-site inside
  the part, which is handled correctly by Core IR. Only `define` inside
  `model` is broken.
- The pre-existing build123d lowerer limitation where function defines
  called in direct geometry produce "Numeric expression `let` is not
  supported by the build123d lowerer." That is a separate backend issue.

## Approach

1. **Compiler guard** — add `"define" => return Err(define_inside_model_error())`
   to the `match head.as_str()` arm in `reject_model_level_sequence_form_group`.
2. **Error message** — clear, actionable, with `let*` example.
3. **System prompt** — `BINDINGS` rule in `TECHNICAL_SYSTEM_PROMPT`.
4. **Docs** — "Common mistake" section in chapter 07 + `public/docs/ecky-ir.md`.

## Proof Gates

- `(define wall 3)` inside model → rejected with `let*` hint (not TypeMismatch).
- `(define half (- frame_length 2))` inside model → rejected with `let*` hint
  (not TypeMismatch).
- `(define (fn args) ...)` at top level → still compiles.
- `(define wall 3)` at top level (literal) → still compiles.
- `cargo test --lib` green for compiler tests (1456 passed, 0 failed).
