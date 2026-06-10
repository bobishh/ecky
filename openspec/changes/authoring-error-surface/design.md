# Design: Self-Teaching Authoring Error Surface

## Existing foundation (do not rebuild)

- `AppError` (`src-tauri/src/contracts/error.rs`) already carries `code:
  AppErrorCode`, `message`, `details`, `operation`, `start_line`, `end_line`,
  and a `diagnostic_context`. Location and op identity already have a home — this
  change adds the missing *layer* and *fix* dimensions.
- `AppResult<T> = Result<T, AppError>` is the universal backend result type.
- The Tauri boundary mandates camelCase serde on boundary structs (`AGENTS.md`).

## Decision 1: a dedicated internal `AuthoringError`, not `Option` fields bolted onto `AppError`

Completeness must be enforced by the type, not by a subjective hotspot list — a
half-covered authoring surface (some errors layered, most not) fails the whole
intent. So authoring failures get their own type:

```rust
pub enum ErrorLayer { Surface, CoreIr, Backend }
pub enum AuthoringReason { ParseSyntax, UnknownOp, Arity, Type, Unsupported, ConstrainedValue }
pub struct ErrorFix { pub hint: Option<String>, pub suggestions: Vec<String> }

pub struct AuthoringError {
    pub layer: ErrorLayer,          // mandatory — which wall
    pub reason: AuthoringReason,    // mandatory — what kind
    pub op: Option<String>,
    pub span: Option<(usize, usize)>,
    pub message: String,
    pub fix: Option<ErrorFix>,
}
```

`layer` and `reason` are non-optional, so the lowering/planning crate — which
returns `Result<_, AuthoringError>` end-to-end — **cannot** produce an unlayered
authoring error. "Every authoring site is tagged" becomes a compiler guarantee
over one bounded crate, replacing "backfill the hotspots and hope."

`AuthoringError` is an **internal Rust type**: it never crosses the Tauri
boundary, so it needs no `specta::Type`/serde derive. Only `AppError` serializes.

## Decision 2: conversion is one-way — `From<AuthoringError> for AppError`, no reverse

Authoring errors flow outward into `AppError` at the command boundary:

```rust
impl From<AuthoringError> for AppError { /* maps layer/reason/op/span/fix */ }
```

There is **no** blanket `From<AppError> for AuthoringError`: it would be lossy —
a generic error has no honest layer. Where pure lowering must call a
generic-`AppError` helper (e.g. reading a referenced SVG asset in
`direct_occt.rs`), resolve it one of two ways, never by a reverse `From`:

- **Hoist the IO out** of pure lowering (read the asset before lowering, pass
  bytes in) — lowering becomes pure transformation, as it should be; or
- **Explicit `.map_err`** into a layered `AuthoringError` — which is *more*
  correct than today's vague `Validation`: a missing referenced asset is a
  `Surface` input error, and now says so.

The size of this friction is bounded by how IO-pure the lowering path is
(observed: mostly transformation + occasional asset reads → tractable). If a
region leans heavily on `AppError` helpers, that shows up as `.map_err` density
in slice 3 — handled point-by-point, never by weakening the one-way rule.

## Decision 3: `AppError` keeps `layer`/`fix` as `Option` — and `None` is correct, not lazy

`AppError` gains `layer: Option<ErrorLayer>` and `fix: Option<ErrorFix>`. `None`
is the *right* value for persistence / provider / internal errors: they are not
authoring failures and have no layer. The `From<AuthoringError>` fills them.
The frontend reads a single type (`AppError`) and shows the layer chip only when
present. Mandatoriness lives on `AuthoringError`, where it belongs.

## Decision 4: layer-primary, `AppErrorCode` derived (the overlap, confronted)

`code` and the proposed layer genuinely overlap (`Parse≈Surface`,
`Render≈Backend`). Rather than declare them "orthogonal forever," on the
authoring path **layer + `AuthoringReason` is primary** and `AppErrorCode`
becomes a coarse, *derived* bucket, filled by the single `From` mapping:

| layer    | AppErrorCode |
| -------- | ------------ |
| Surface  | Parse        |
| CoreIr   | Validation   |
| Backend  | Render       |

No new `AppErrorCode` variants; the enum is unchanged and simply populated by
the mapping. Non-authoring errors keep choosing their `code` directly as today.

## Decision 5: finite Core IR vocabulary powers "did you mean"

Unknown-op errors (`AuthoringReason::UnknownOp`) compute nearest valid ops from
the Core IR op registry via a small edit-distance, feeding `fix.suggestions`
(`bx` → "did you mean `box`?"). Pure function, unit-tested in isolation. This is
where the finite vocabulary finally pays the *user*, not the engineer.

## Decision 6: layer is chosen at the failing site

No global inference. Parse/read of the surface → `Surface`; op/selector not in
the Core IR set → `CoreIr` (+ suggestions); op that lowered but the active
backend cannot execute → `Backend` (name the backend; offer an alternative).

## Frontend rendering

Authoring errors reach the user as `AppError` through the Ecky bubble
(`sessionError` / `threadError` in `src/lib/agents/draftFeedback.ts`) and the
code panel. When `layer`/`fix` are present, render a layer chip (SURFACE /
CORE IR / BACKEND) and a fix line (`hint` + `suggestions`). The raw `message` is
never hidden (`AGENTS.md` "Real Error Reporting"). When absent, render exactly
as today.

## Docs band-aid retirement

Once errors carry the layer, the "How Ecky Thinks" debugging heuristic
(`public/docs/ecky-ir.md`, ~lines 7-17) is redundant. Trim it to a one-line
notation note (parens compile to a fixed op set; default render is exact B-rep).
This restores `sections[0] = "First Solid: Ball on a Base"`, resolving the
pre-existing `src/lib/docs/eckyIrGuide.test.ts` failure without editing the
test's expectation. Supersedes the standalone "fix eckyIrGuide" task.

## Files touched

- `src-tauri/src/contracts/error.rs` — `ErrorLayer`, `AuthoringReason`,
  `ErrorFix`, `AuthoringError`, `AppError.layer/fix`, `From<AuthoringError>`.
- new `src-tauri/src/ecky_ir/op_suggest.rs` — nearest-op suggester.
- `src-tauri/src/ecky_cad_host/direct_occt.rs`, `direct_occt_executor.rs`,
  `ecky_ir/*` lowering — return `AuthoringError`; IO-hoist / `.map_err` per
  Decision 2.
- `src/lib/agents/draftFeedback.ts` (+ test) and the error bubble/code panel.
- `public/docs/ecky-ir.md` + `src/lib/docs/eckyIrGuide.test.ts`.

## Open question (defer)

- Whether the agent (MCP) should receive `fix`/`suggestions` to self-correct
  without a round-trip. The structure allows it; out of this change's gates.
