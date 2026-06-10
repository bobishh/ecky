# Tasks: Self-Teaching Authoring Error Surface

BDD dual-loop. Failing test first (red), minimum code (green), refactor green.
Backend slices run `cd src-tauri && cargo test`; frontend slices run
`npm run test:unit` / targeted Playwright. Run the relevant suite after each
green, the full set before the final checkpoint.

## Composition invariants (hold across every slice)

- Conversion is one-way: `From<AuthoringError> for AppError` only. Never add a
  reverse `From<AppError> for AuthoringError`.
- The lowering/planning crate returns `Result<_, AuthoringError>` end-to-end.
- `AuthoringError` is internal: no `specta::Type`/serde. Only `AppError` crosses
  the boundary.
- `AppError.layer/fix` are `Option`; `None` means "not an authoring error" and
  is correct for persistence/provider/internal — do not retrofit those.

## 1. Error types + one-way conversion (foundation, not delegable)

Write scope:

- `src-tauri/src/contracts/error.rs`

Tasks:

- [ ] 1.1 (red) Unit test: `From<AuthoringError>` maps layer→code per the table
  (Surface→Parse, CoreIr→Validation, Backend→Render) and carries op/span/fix.
- [ ] 1.2 (red) Unit test: an `AppError` JSON payload omitting `layer`/`fix`
  deserializes with both absent (non-breaking boundary).
- [ ] 1.3 (red) Unit test: `AppError` with a layer + fix serializes camelCase and
  round-trips.
- [ ] 1.4 (green) Add `ErrorLayer`, `AuthoringReason`, `ErrorFix`,
  `AuthoringError` (layer/reason mandatory; internal, no serde), and
  `AppError.layer: Option<ErrorLayer>` + `AppError.fix: Option<ErrorFix>`
  (optional camelCase serde).
- [ ] 1.5 (green) Implement `From<AuthoringError> for AppError` (the only
  conversion direction).
- [ ] 1.6 (refactor) Layer-aware constructors on `AuthoringError`
  (`surface/core_ir/backend`) taking reason + optional fix.

## 2. Nearest-op suggester (delegable — isolated module)

Write scope:

- new `src-tauri/src/ecky_ir/op_suggest.rs` (+ `#[cfg(test)]`)

Tasks:

- [ ] 2.1 (red) Unit test: a near-miss (`bx`) returns the nearest valid Core IR
  op (`box`) within threshold.
- [ ] 2.2 (red) Unit test: a far-off name returns no suggestion.
- [ ] 2.3 (green) Edit-distance nearest-op lookup over the Core IR op registry;
  pure function returning `Vec<String>`.
- [ ] 2.4 (refactor) Source the op list from the existing registry, not a copy.

## 3. Lowering returns AuthoringError (integrative — keep on main thread)

Write scope:

- `src-tauri/src/ecky_cad_host/direct_occt.rs`
- `src-tauri/src/ecky_cad_host/direct_occt_executor.rs`
- `src-tauri/src/ecky_ir/*` lowering (unknown op / arity / type / parse sites)

Tasks:

- [ ] 3.1 (red) Test: unknown op → `layer = CoreIr`, `reason = UnknownOp`, op
  named, nearest-op suggestion present.
- [ ] 3.2 (red) Test: op unsupported by active backend → `layer = Backend`,
  `reason = Unsupported`, backend named, fix hint present.
- [ ] 3.3 (red) Test: constrained-value site (axis) → `reason = ConstrainedValue`,
  fix lists the valid set.
- [ ] 3.4 (red) Test: surface parse failure → `layer = Surface`, span set.
- [ ] 3.5 (green) Convert lowering signatures to `Result<_, AuthoringError>`;
  resolve generic-`AppError` call sites by IO-hoist or explicit `.map_err`
  (never a reverse `From`); boundary returns `AppError` via the `From`.
- [ ] 3.6 (refactor) Fold repeated layer/reason/fix construction into helpers.

## 4. Frontend renders layer + fix (delegable after slice 1 — disjoint from Rust)

Write scope:

- `src/lib/agents/draftFeedback.ts` (+ `.test.ts`)
- error bubble / code panel rendering

Tasks:

- [ ] 4.1 (red) Unit test: presentation exposes a layer chip + fix line when the
  error carries them; falls back cleanly when absent.
- [ ] 4.2 (green) Thread layer + fix through; render chip + fix line; keep the
  raw message visible.
- [ ] 4.3 (red/green) Playwright: an authoring-error bubble shows the layer chip
  and a suggestion; raw message still present.

## 5. Retire the docs band-aid (delegable — fully isolated)

Write scope:

- `public/docs/ecky-ir.md`
- `src/lib/docs/eckyIrGuide.test.ts`

Tasks:

- [ ] 5.1 (green) Trim "How Ecky Thinks" to a one-line notation note; no up-front
  architecture section before the first model.
- [ ] 5.2 (green) Confirm `eckyIrGuide` parses `sections[0] = "First Solid: Ball
  on a Base"`; the previously failing assertion passes without editing the
  expectation. Supersedes the standalone "fix eckyIrGuide" task.

## 6. Checkpoint

- [ ] 6.1 `cd src-tauri && cargo test` green for new/affected modules.
- [ ] 6.2 `npm run test:unit` green (including restored `eckyIrGuide`).
- [ ] 6.3 Targeted Playwright green.
- [ ] 6.4 `cd src-tauri && cargo check` clean.
