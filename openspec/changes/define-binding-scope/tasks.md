# Tasks: Define Binding Scope

BDD dual-loop. Failing test first (red), minimum code (green), refactor green.
Backend slices run `cd src-tauri && cargo test`.

## 1. Compiler guard

Write scope:

- `src-tauri/src/ecky_scheme/compiler.rs`

Tasks:

- [x] 1.1 (red) Unit test: `(define wall 3)` inside model → error containing
  `(define ...)` and `let*`.
- [x] 1.2 (red) Unit test: `(define half (- frame_length 2))` inside model →
  error containing `let*`, NOT containing `TypeMismatch`.
- [x] 1.3 (green) Unit test: `(define (fn args) ...)` at top level → compiles.
- [x] 1.4 (green) Unit test: `(define wall 3)` at top level → compiles.
- [x] 1.5 (green) Add `"define"` arm to `reject_model_level_sequence_form_group`
  with `define_inside_model_error()` message.

## 2. System prompt

Write scope:

- `src-tauri/src/lib.rs` (`TECHNICAL_SYSTEM_PROMPT`)

Tasks:

- [x] 2.1 Add `BINDINGS` rule: never use `define` inside `model`, use `let*`
  inside `part` for derived values, top-level function defines are OK.

## 3. Documentation

Write scope:

- `docs/books/ecky-ir/chapters/07-repetition.md`
- `public/docs/ecky-ir.md`

Tasks:

- [x] 3.1 Add "Common mistake: `(define ...)` inside `(model ...)`" callout
  with wrong/right examples.
- [x] 3.2 Rebuild book (`npm run build:book`).

## 4. Checkpoint

- [x] 4.1 `cargo test --lib` green (compiler tests: 107 passed, 0 failed;
  full suite: 1456 passed, 0 failed excluding pre-existing freecad stack overflow).
- [x] 4.2 `cargo check` clean.
- [x] 4.3 CLI `ecky check` produces the new error on all define-inside-model cases.
