# Ecky IR Field Guide — Revision Plan

Historical review. The current illustrated book, source ownership and verified
deployment instructions are described in [PUBLISHING.md](PUBLISHING.md).
Do not use the old chapter layout below as the current build graph.

Plan for reworking the field guide (`docs/books/ecky-ir/`). Lighter than an
OpenSpec change — this is docs, not code. Work it top to bottom; check boxes as
you go. Each task is small enough to land in one pass.

Source of the review (2026-06): the book has a solid example-driven skeleton
but (a) misses flagship features that already ship, (b) documents one feature
that does not compile, (c) has a numbering footgun, and (d) reads dry.

## Single-source rule

`docs/books/ecky-ir/ecky-ir-corpus.md` is canonical for campaign lessons,
human language reference, and bounded agent reference. Projection scripts
publish `public/tutorials/ecky-campaign.md`, `public/docs/ecky-ir.md`, and
`public/docs/ecky-agent-reference.md`. The paged HTML site remains shell,
routing, and Markdown rendering only. Files under
`docs/books/ecky-ir/chapters/` are generated split views; never edit generated
projections directly.

`npm run build:book` synchronizes the split views. `npm run check:book-source`
fails when any generated chapter drifts. The operation catalogue remains
generated from `ecky_language_surface`, then joins the canonical agent body in
the API/MCP prompt builder.

## Ground truth verified by compilation (do not re-decide)

- `units` (`12mm`, `45deg`, ...) — **implemented**, compiles. Currently only a
  6-line stub in ch03.
- `tag-face` / `(tag ...)` selector — **implemented**, compiles. Documented in
  ch05 (good).
- `:created-by <shape>` — **implemented, native-only by design**. The native
  OCCT planner resolves it into an originating-slot reference (green test
  `plans_created_by_keyword_into_direct_occt_slot_reference`). FreeCAD interop
  rejects it because it has no slot-provenance index. The CLI validates Ecky
  source directly; the removed build123d runtime no longer participates.
- `define-component` — implemented; only a cameo in ch07.
- component library MCP (`component_extract`/`search`/`get`) — implemented,
  **0 book coverage**.
- project folders (`project_folder_export`/`status`/`apply`, FILE↗, watcher) —
  implemented, **0 book coverage**.
- ~~`ecky check` CLI returns exit 0 even when it prints a lowering error~~ —
  **NOT reproducible** (re-verified 2026-06): `ecky check` exits 1 on every
  error path tested (`:created-by` rejection, unknown component key, closedness
  violation) and exit 0 only on clean compiles. The earlier observation was a
  misdiagnosis. No CLI bug to fix.

## Known follow-up (discovered, not a book-content task)

- **Book image assets are committed snapshots.**
  `npm run build:mission-assets` regenerates current mission images through the
  native OCCT preview path. The removed Python/build123d renderer is not part of
  docs generation.

## Tasks

### A. Reality fixes (do first — stop teaching broken/missing things)

- [x] A1. `:created-by` in `05-round-shell-select.md`: KEEP the example (it
  works), but frame it as a **native-only** provenance selector. Add a callout:
  it resolves on the native OCCT backend (the default); FreeCAD interop rejects
  it because it lacks a slot-provenance index. Do NOT remove it and do NOT
  weaken interop to accept it — native-only is the intended design. (Confirmed
  by the green native planner test.)
- [x] A2. CLOSED — not a bug. Re-verified `ecky check` returns exit 1 on error
  and exit 0 on success across multiple failure paths. The earlier exit-0
  observation did not reproduce; nothing to file. (Examples can be CI-validated
  with `ecky check` as-is.)

### B. New chapters (the big content gaps)

- [x] B1. New chapter **"Components & Reuse"** (DONE: lesson chapter added after
  ch07; example compiles via `ecky check`. NOTE: the appendix already had a
  dense `define-component` *reference* + MCP loop — the new chapter is narrative
  teaching that links to it, not a duplicate.) Original spec:
  New chapter **"Components & Reuse"** (insert after ch07 repetition).
  Cover: `define-component` (signature, defaults, keyword overrides),
  closedness rule (body sees only signature + local bindings), verify-travel
  (`partkey/tag`), and the MCP library loop `component_extract → component_search
  → component_get → paste + instantiate`. One worked example: lift a proven
  part into a component, reuse it across two parts. Tone: problem → reuse → proof.
- [x] B2. New chapter **"Projects as Folders"** (DONE: lesson chapter before the
  final model; watcher facts verified against `project_folder.rs` — polling,
  two-tick settle, `folder-sync` auto-apply, failures memoized once. Dropped the
  unverified "FILE↗ button" claim — no such control found in the Svelte UI.)
  Original spec:
  New chapter **"Projects as Folders"** (workflow chapter, near the end
  before the final model). Cover: `project_folder_export` mirrors `model.ecky`,
  edit in any editor / LLM file skill, watcher re-renders as a new version,
  the sync states (clean/fileChanged/threadAdvanced/conflict), FILE↗ button,
  and the rule "the folder is a mirror; threads/versions stay canonical".
- [x] B3. New opening chapter **"How Ecky Thinks"** (chapter 00 / intro). Short
  mental model: `.ecky` is a Scheme surface → lowered to a finite Core IR →
  rendered by an exact solid kernel (native OCCT) or interop backends. Anchor
  the whole book so examples have a frame. Keep it one screen.

### C. Expand thin coverage

- [x] C1. (DONE: rewrote the suffix stub into real teaching — base units mm/deg,
  conversion table verified by lowering (`1in`→25.4, `1.5708rad`→90), unitless
  cases, and the honest caveat that Ecky does NOT type-check dimensions
  (`45deg` in a width slot compiles to a 45 mm width — verified).) Original spec:
  Grow the **units** section in `03-parameters.md` from a suffix list
  into real teaching: what stays unitless (counts, ratios, segment counts),
  dimension-mismatch behavior, and the physical-authoring discipline (lengths
  in mm/cm/in, angles in deg/rad). It compiles — treat it as a first-class
  feature, not a footnote.

### D. Structure

- [x] D1. DONE: `git mv` complex-film-adapter to `11-`, updated the asset ref in
  both source copies + `index.md`, and renamed the generated PNG artifacts
  10-→11- (PNGs are gitignored build artifacts; CI regenerates from the new
  filename). Verified: built HTML references `11-...png` (no `10-` left), asset
  present in EPUB OEBPS, book test green, and the chapters/ glob now sorts
  correctly (`10-real` → `10a-projects` → `11-complex`), killing the footgun.
  Historical note: this rename originally reused the byte-identical artifact.
  Current mission assets are regenerated through native OCCT.
  Original spec:
  Renumber the two ch10s: `10-real-model-patterns.md` stays **10**,
  `10-complex-film-adapter.md` becomes **11** (it is literally "the final
  model", and real-patterns says "before the final adapter"). Update
  `index.md` and any `render-source`/build references.
  COST NOTE (verified): the chapter filename was the asset stem for generated
  image names. Renaming the file to `11-` also requires renaming the committed PNG
  `assets/10-complex-film-adapter-01.png` → `11-...`, updating its reference in
  `public/docs/ecky-ir.md:633` and the chapters mirror, then regenerating
  through the render stack. Served order is ALREADY correct (real-patterns
  precedes final-model in `public/docs`), and users see chapter titles, not the
  number — so this is cosmetic/maintenance only. Do it as its own focused pass
  with the renderer available; do not rush it mid-context.
- [x] D2. Re-check `index.md` chapter order after B1/B2/B3/D1 land so the TOC
  matches the new numbering and the build (`src/lib/docs/eckyIrBook.ts`) still
  produces a chapter per entry.
- [x] D3. Optional: ch05 carries 5 concepts (fillet/chamfer/shell/tag/created-by).
  Either split into two chapters or accept it as an "intermediate" chapter and
  say so in its intro. Low priority — do after B/C.

### E. Tone pass (the "суховат" fix)

- [x] E1. Rewrite intros of `01`–`04` to add motivation and connective tissue:
  each chapter opens with a small problem, hits a wall, resolves it (borrow the
  red→green tension that makes ch09 the best chapter). Add one-line bridges
  between chapters ("now that you can X, you'll hit Y").
- [x] E2. Add a short "what goes wrong" / gotcha note to chapters that lack one
  (selectors breaking after booleans, fillet radius vs face width, mixing
  mesh-only and exact ops). Failure modes are where learning lives.
- [x] E3. Keep code/terms exact; only the prose gets voice. Match the
  personality of AGENTS.md and the specs — the book is currently the flattest
  writing in the repo.

## Suggested order

A1 → B3 (intro anchors everything) → B1 → B2 → C1 → D1/D2 → E1/E2 → D3/E3.

## Commit guidance

Each landed chunk is a conventional commit, e.g.
`docs(book): add Components & Reuse chapter`,
`docs(book): renumber final-model chapters`,
`fix(book): drop unimplemented :created-by example`.
After content changes, rebuild the book (`scripts/build_ecky_ir_book.ts`) and
run the book unit test (`src/lib/docs/eckyIrBook.test.ts`).
