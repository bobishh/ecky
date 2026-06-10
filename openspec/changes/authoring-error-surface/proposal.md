# Proposal: Self-Teaching Authoring Error Surface

## Intent

Make authoring errors teach. Today, understanding *why* a model failed requires
a docs lecture ("How Ecky Thinks" — the three-layer surface / Core IR / backend
model) read before the first model. That a tutorial must explain how to decode
failures is a symptom: the error surface itself is mute. The fix is not more
documentation — it is errors that name which layer failed, what failed, why, and
what to do next, so the layer model lands through the errors a user actually
hits instead of an up-front architecture tour.

## Problem (grounded in the code)

- `AppError` (`src-tauri/src/contracts/error.rs`) is already structured: `code`,
  `message`, `details`, `operation`, `start_line` / `end_line`,
  `diagnostic_context`. But it has **no layer** (which of surface / Core IR /
  backend owns the failure) and **no structured fix / suggestions**. "Which
  wall did I hit?" is unanswerable from `code` alone (`Validation` spans all
  three layers).
- Error sites lower through ad-hoc prose: `AppError::validation(format!(...))`
  scattered across `direct_occt.rs`, `direct_occt_executor.rs`, and the
  `ecky_ir/*` lowering. Quality varies by author; the existing structured fields
  (`operation`, line span) are frequently left unset.
- One good exemplar exists — the executor's mirror-axis error names the bad
  value and the valid set ("unknown axis; use `x`, `y`, or `z`"). Most sites do
  not reach this bar.
- The Core IR vocabulary is **finite**. That is the docs' bragging point, but it
  currently pays the engineer (reproducibility) and not the user. A finite
  vocabulary is exactly what makes "did you mean `cube`?" suggestions trivial.

## Scope

- Introduce a dedicated internal `AuthoringError` (mandatory `layer` ∈ surface /
  coreIr / backend, mandatory `reason`, plus op / span / fix) for the
  lowering/render authoring path. Completeness is a compiler guarantee: the
  lowering crate returns `Result<_, AuthoringError>`, so no authoring failure can
  be unlayered.
- Add `layer` + `fix` to `AppError` as optional camelCase fields, filled only via
  a one-way `From<AuthoringError> for AppError`. `None` means "not an authoring
  error" (correct for persistence/provider/internal).
- Compute "did you mean" suggestions for unknown operations from the finite Core
  IR op set (edit-distance nearest neighbours).
- Render `layer` + `fix` in the frontend error surface (Ecky bubble + code panel),
  distinct from the raw message.
- Once errors teach the layer, retire the docs band-aid: trim "How Ecky Thinks"
  to a one-line notation note and restore the `eckyIrGuide` section test.

## Out of Scope

- Re-typing non-authoring errors (persistence / provider / internal). They keep
  generic `AppError` with `layer = None` — that is the correct value, not a
  stopgap.
- Error sites outside the lowering/render authoring path. Completeness on the
  authoring path is enforced by the `AuthoringError` type (every authoring site
  must produce a layer), not by a subjective hotspot list.
- Adding `AppErrorCode` variants. On the authoring path `layer` + `reason` is
  primary; the existing `AppErrorCode` is populated as a derived coarse bucket
  (Surface→Parse, CoreIr→Validation, Backend→Render).
- A reverse `From<AppError> for AuthoringError` — it is lossy and forbidden.
- LLM-generated error explanations.
- Internationalization of error text.
- Converting `#[cfg(test)]` `panic!` asserts (those are tests, not the surface).

## Approach

- Add `ErrorLayer`, `AuthoringReason`, `ErrorFix`, and the internal
  `AuthoringError` (no serde — never crosses the boundary) plus the optional
  `AppError.layer/fix` and the one-way `From` in `contracts/error.rs`, camelCase
  per the Tauri boundary.
- Convert the lowering/render path to return `AuthoringError`; resolve any
  generic-`AppError` call sites inside it by hoisting IO out or an explicit
  `.map_err` into a layered `AuthoringError` (never a reverse `From`).
- Add a pure nearest-op suggester over the Core IR op registry.
- Frontend: extend the error presentation (bubble/code panel) to show a layer
  chip and a fix line, reading the new optional fields.

## Proof Gates

- An unknown operation fails with `layer` set, the offending op named, and at
  least one nearest valid-op suggestion when one exists.
- An operation that lowers but the active backend cannot execute fails with
  `layer = backend`, names the backend, and offers a fix (alternative op or
  backend switch).
- A surface parse failure fails with `layer = surface` and a line span.
- The frontend renders the layer and fix distinctly from the raw message.
- Targeted authoring sites return a structured `AppError` (with layer) on bad
  input, not a panic.
- "How Ecky Thinks" is reduced to a notation note without losing the lesson, and
  the `eckyIrGuide` section test passes against the trimmed document.
- `cargo test` (src-tauri), `npm run test:unit`, and targeted Playwright pass.
