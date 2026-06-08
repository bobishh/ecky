# Tasks: CAD Transpile (thin, over the existing agent/LLM path)

Transpile is a translate-intent request over the existing system prompt
(`agent_language_reference`), LLM path (`llm.rs` + `Config`), version writing, and
verify gate. New code is limited to the translate instruction, two thin
affordances (code-window toggle, MCP convention), source adapters (extraction
only), and an optional dev CLI. Author tests first (BDD red→green) per task.

## 1. Translate instruction + message builder (pure, tested)

- [ ] 1.1 (test) `build_transpile_messages(source, backend)` returns
  `(system, user)` where `system == agent_language_reference(backend)` and `user`
  contains both the fixed translate preamble and the source verbatim.
- [ ] 1.2 Implement the builder + the fixed preamble (parametrize, loop-ify,
  "output only Ecky"). No model code here — pure string assembly.
- [ ] 1.3 (test) preamble carries the semantic ask (params/loops) and forbids
  prose-only output; drift guard ties system prompt to `agent_prompt`.

## 2. LLM call (reuse, NIM-capable)

- [ ] 2.1 Resolve provider/model/key/base_url from `Config` first, env override
  second (`NVIDIA_API_KEY`/`NIM_API_KEY`, `…_BASE_URL`, `…_MODEL`). Default
  base_url = NVIDIA NIM (`https://integrate.api.nvidia.com/v1`).
- [ ] 2.2 Call via the existing OpenAI-compatible path (`send_openai_request` +
  `extract_openai_message_content`); do not add a second HTTP client.
- [ ] 2.3 Strip any non-Ecky wrapping (code fences / prose) from the model reply
  before compile; (test) fenced and bare replies both yield clean source.

## 3. Dev CLI (optional harness — model/provider comparison)

- [ ] 3.1 Bin `cad_to_ecky <input> [--backend] [--model] [--base-url] [--out]`
  mirroring `translate_legacy_python_to_ecky_ir`'s arg shape.
- [ ] 3.2 `--dump-prompt` prints the assembled system+user with no API call
  (free inspection / diffing across models).
- [ ] 3.3 (test) arg parsing + `--dump-prompt` output is deterministic and
  network-free.

## 4. Source adapters (extraction only; never emit Ecky)

- [ ] 4.1 OpenSCAD `.scad`: pass-through text. (test) round-trips unchanged into
  the user message.
- [ ] 4.2 FreeCAD `.fcstd`: a fresh freecadcmd extractor that dumps the feature
  tree to JSON (extraction only — no Ecky emission). The JSON is the source text.
- [ ] 4.3 STEP/BREP `.step`: textual summary (bodies, dims, key features) as
  source text. Faithfulness is then enforced by the parity gate, not the adapter.
- [ ] 4.4 Adapter dispatch by extension/sniff; unknown → treat as raw text.

## 5. Verify gate + repair loop

### V1 (UI / consumer) — model + dialogue verify
- [ ] 5.1 After transpile, compile + render + `verify_generated_model` (structural
  + model-authored `(verify …)`) as normal; never auto-commit a red result.
- [ ] 5.2 Dialogue-accrued verify: when the user states a requirement ("ears
  separate"), the model adds the matching `(verify …)` clause **with** the
  geometry change, so it persists as a check. Size/intent errors that pass
  structural checks are caught by the human in the loop, not by source parity.
- [ ] 5.3 Repair loop: feed the compiler/verify diagnostic back to the model (per
  the API operating contract) and re-request, capped; report capped red honestly
  without commit.

### Internal CLI only (proving-ground, NOT the UI)
- [ ] 5.4 Source parity: where the source is measurable, compare source bbox+volume
  (source → STEP/STL → `import_step`) to the rendered Ecky within tolerance;
  surface a mismatch as a parity diagnostic. Auto-catches size errors (the 2×
  head). Source-specific runtimes (freecadcmd, STL/STEP measurer) live behind the
  CLI; used to vet components before release. Do NOT wire into the UI.

## 6. Surfaces

- [ ] 6.1 Code window (V1 only): `translate to Ecky` toggle/action sends the
  current buffer as the source and replaces it with the result; keep the original
  recoverable on failure (no silent clobber of a good buffer). No FreeCAD or source
  parity in the UI.
- [ ] 6.2 MCP: document the message convention (send foreign code + ask to
  transpile → agent writes a new thread version). Optionally add a thin
  `cad_transpile` verb that canonicalises the translate instruction; it must be
  pure sugar over the existing authoring flow.

## 7. Optional: stdlib seeding consumer

- [ ] 7.1 A transpiled + curated + verified model may go through
  `component_extract --save` like any other; no bespoke path. Stdlib's source of
  truth stays the hand-authored curated set (`language-convenience-stdlib`).

## Migration note

The deterministic-emitter prototype and the library-survey script
(`scripts/freecad-transpiler/`) have been removed — their concept is dead. The
kill evidence (0% Array/expression/spreadsheet, 45% PartDesign over 97 mechanical
parts) is recorded in the proposal. The FreeCAD adapter (task 4.2) is built fresh
as extraction-only when needed; the LLM owns all Ecky emission.
