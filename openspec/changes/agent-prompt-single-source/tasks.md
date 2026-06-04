# Tasks: Single-Source Language Reference → Three Artifacts

## 1. Single source + shared builder

- [ ] 1.1 Designate `public/docs/ecky-ir.md` + `surface-reference` as the single
  language source; document it in the book's revision notes and `design.md`.
- [x] 1.2 `agent_language_reference(backend)` builder — DONE. New module
  `src-tauri/src/agent_prompt.rs`: assembles `API_OPERATING_CONTRACT` +
  `ecky_source_guide_text()` + injected op catalogue. One function, will be
  consumed by both MCP and API (wiring = task 3).

## 2. API system prompt generation

- [~] 2.1 REVISED decision: do NOT mechanically distill the human book into the
  prompt. The human book (warm, images, teaching) and the agent body genuinely
  want different prose — confirmed by the 8K budget (the 68 KB book cannot fit).
  Instead the **language body = the existing concise agent guide**
  (`ecky_source_guide_text()`, ~10 KB), and the **op catalogue is the shared
  auto-source** (surface-reference). So "one source → 3 artifacts" = the op
  catalogue is shared+drift-checked across book and prompt; the prose is two
  tuned versions, not one mechanical distillation. (Image-strip + drift checks
  still enforced.) The book-side drift check is task 4.3.
- [x] 2.2 Inject op catalogue from `surface-reference` — DONE, **as
  documentation-by-example** (per review: LLMs author better from a commented
  example than from prose, and it is terser). `op_catalogue()` emits a `scheme`
  code block of one real `.ecky` snippet per form (the entry's `example` field)
  with the `description` as a trailing `; comment`; restriction note only for
  backend-restricted ops. Never hand-written → cannot drift. Net effect: smaller
  AND more useful (build123d prompt 31 KB → 24 KB).
  DISCOVERED REAL DRIFT (separate fix): the Phase-1 convenience ops (torus,
  ellipse, regular-polygon, slot ×4, trapezoid, wedge, thread, rib, groove) are
  **missing from `ecky_language_surface::CAD_OPS_PORTABLE`** — the agent did not
  know they existed. Registering them belongs with the ops themselves
  (`language-convenience-stdlib`, gated by the `cad::MODULE.exports` test), so it
  ships there, not in this builder commit. (`draft` also pending: native+build123d
  partial support fits none of the portable/exact-only/rust-only buckets — needs
  a support-category follow-up.)
- [x] 2.3 API operating contract — DONE. `API_OPERATING_CONTRACT` const: `.ecky`
  only, no tools, mm/deg, code→diagnostic→retry, respect per-op backend support.
- [x] 2.4 Budget guard — DONE. `AGENT_PROMPT_CHAR_CEILING = 32_000` (~8K tokens);
  test `agent_prompt_stays_within_budget` across all 3 backends (EckyRust is the
  largest at ~31 KB after the per-line support-string trim).
- [ ] 2.5 Emit from `build:book` (single pipeline with the EPUB/HTML) into a
  **committed** `.md` under `docs/` (reviewable in diffs; the EPUB stays
  gitignored).
- [ ] 2.6 Golden-file test of the generated prompt.
- [ ] 2.7 CI freshness gate: regenerate the prompt and fail if it differs from the
  committed copy (checked-in generated file cannot go stale).

## 3. Wire MCP + API to the shared builder

- [ ] 3.1 Route `ecky://guides/technical-system-prompt` (and the language guide)
  through `agent_language_reference()` so MCP serves the same body.
- [ ] 3.2 API mode consumes the generated prompt artifact directly as system prompt.
- [ ] 3.3 Assert MCP-served language body == API prompt language body (no fork).

## 4. Drift check

- [ ] 4.1 Test: every op named in the generated prompt ∈ `surface-reference`.
- [ ] 4.2 Test: every `surface-reference` op appears in the prompt op table.
- [ ] 4.3 Test: book appendix op list == `surface-reference`.
- [ ] 4.4 Wire the drift tests into CI so a new/renamed op must appear in all
  three artifacts or the build fails.

## 5. Self-containment guard

- [ ] 5.1 Test: the API prompt references no MCP tool name and contains no image
  markup (it must stand alone for a tool-less agent).
