# Proposal: Language Maturity (units, selectors, views, diagnostics, verify-TDD)

## Intent

Take `.ecky` from "convincing IR with a friendly surface" to a language that
survives large real models, addressing the five reviewer findings plus the
verify-first authoring loop:

1. Reuse/abstraction pressure (repetition across parts).
2. Units and lightweight dimensional types.
3. Topology-stable face/edge selectors (tags + provenance).
4. Assembly/view separation from manufacturing geometry.
5. CAD-grade diagnostics with physical meaning and live parameter values.
6. Verify-TDD: generation writes `(verify ...)` together with the model and
   iterates red -> green before presenting a result.

Everything stays additive: existing sources compile unchanged, Core IR keeps
its shape where possible, and each capability lands behind its own task gate.

## Findings vs. current state (complexity honest-check)

1. Reuse — **mostly built already**: model-level `let`/`let*` splice, helper
   `define` (+ inline expansion), `define-component` with closed signatures,
   component library. Remaining work is teaching (book/guides) plus closing
   gaps found while documenting. Size: S.
2. Units/types — `:unit` already parses on params. Missing: literal suffixes
   (`70mm`, `45deg`, `0.5rad`), unit normalization, and a dimension table for
   op signatures so the verifier can reject `(rotate plate_w ...)`. Size: M
   for suffixes + normalization, L for full dimensional checking.
3. Selectors — stable/durable target ids and selector payloads exist; missing
   are *authored* semantics: `(tag-face name selector geometry)` persisting a
   named anchor into the manifest, and `:created-by <shape>` provenance from
   `build` bindings. Size: M for tags, L for provenance (touches the planner
   and the native runner's topology report).
4. Assembly/view — debug-overlay precedent already enforces "preview never
   exports"; `view` generalizes it (named, preview-only placement layers) and
   `assembly`/`export` formalize what component packages half-do. Size: M.
5. Diagnostics — error culture exists (raw bodies, spans, advisories); missing
   is parameter-resolved context ("self-intersects at rail_tip_w=8") emitted
   from normalize/plan/runner failures. Size: M, spread across layers.
6. Verify-TDD — `verify` clauses, structural verification,
   `verify_generated_model`, and a dormant `max_verify_attempts` loop all
   exist. Missing: prompts/guides that demand verify-first authoring, a
   feedback formatter that turns red metrics into a next-attempt prompt, and
   red/green surfacing in the UI. Size: M (mostly prompt + loop glue).

## Scope

- `let`-reuse documentation pass plus any compiler gaps it exposes (G1).
- Unit literal suffixes lowered in the existing source pre-pass; canonical
  internal units (mm, deg); op dimension table + verifier check behind a
  strictness toggle (G2).
- `(tag-face ...)`/`(tag-edge ...)` authored anchors resolved at plan time to
  stable target ids, persisted in the manifest, usable anywhere a selector
  string is accepted; `:created-by` provenance selector as a follow-up task
  over `build` shape lineage (G3).
- `(view name ...)` preview-only placement layers (exploded views) with the
  debug-overlay export gate; `(assembly ...)`/`(export ...)` clauses kept
  spec-only until the view layer proves the gate (G4).
- Diagnostic enrichment: every normalize/plan/export error carries part key,
  op, source span, and the resolved parameter values that produced it (G5).
- Verify-TDD loop: system prompt + authoring card + book updates demanding
  model+verify in one shot; generation loop renders, runs authored verify,
  and on red feeds formatted metric deltas back for up to
  `max_verify_attempts` retries (default raised from 0); UI shows per-tag
  red/green chips on the version (G6).

## Out of Scope

- Full static type system or inference beyond the dimension table.
- Geometry-solver constraints (mate/joint solving) — assemblies stay
  placement-based as today.
- Breaking changes to existing sources, Core IR public structs, or stable
  node keys (same locks as component-unification).
- Automatic topology re-binding after arbitrary edits (tags reduce breakage;
  they do not promise it away).

## Success Criteria

- A book-scale model can hoist its derived values once and reuse them across
  parts without copy-paste, documented end to end.
- `(box 70mm 42mm 4mm)` compiles; `(rotate 45mm ...)` is a compile error with
  the offending span when strict units are on.
- A fillet bound to `(tag-face mounting_top ...)` survives a parameter sweep
  that changes face indexing.
- An exploded `view` never leaks into STL/STEP exports (gate-tested).
- A boolean failure names the part, op, and live parameter values.
- A "make X" prompt produces model+verify together, and the app retries red
  verifies automatically before showing the result, with red/green visible.
