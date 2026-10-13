# Tasks: CAD Transpile (thin, over the existing agent/LLM path)

Transpile is a translate-intent request over the existing system prompt
(`agent_language_reference`), LLM path (`llm.rs` + `Config`), version writing, and
verify gate. New code is limited to the translate instruction, two thin
affordances (code-window toggle, MCP convention), source adapters (extraction
only), and an optional dev CLI. Author tests first (BDD red→green) per task.

## 1. Translate instruction + message builder (pure, tested)

- [x] 1.1 (test) `build_transpile_messages(source, backend)` returns
  `(system, user)` where `system == agent_language_reference(backend)` and `user`
  contains both the fixed translate preamble and the source verbatim.
- [x] 1.2 Implement the builder + the fixed preamble (parametrize, loop-ify,
  "output only Ecky"). No model code here — pure string assembly.
- [x] 1.3 (test) preamble carries the semantic ask (params/loops) and forbids
  prose-only output; drift guard ties system prompt to `agent_prompt`.

## 2. LLM call (reuse, NIM-capable)

- [x] 2.1 Resolve provider/model/key/base_url from `Config` first, env override
  second (`NVIDIA_API_KEY`/`NIM_API_KEY`, `…_BASE_URL`, `…_MODEL`). Default
  base_url = NVIDIA NIM (`https://integrate.api.nvidia.com/v1`). Evidence:
  `cad_transpile::tests::resolve_uses_selected_config_engine_then_documented_nim_environment_overrides`
  and `resolve_defaults_empty_configured_base_url_to_nvidia_nim`.
- [x] 2.2 Call via the existing OpenAI-compatible path (`send_openai_request` +
  `extract_openai_message_content`); do not add a second HTTP client. Evidence:
  `transpile_via_openai_compatible` accepts the shared client; payload shape is
  pinned by `cad_transpile::tests::transpile_payload_uses_openai_messages_with_the_resolved_model`.
- [x] 2.3 Strip any non-Ecky wrapping (code fences / prose) from the model reply
  before compile; (test) fenced and bare replies both yield clean source. Evidence:
  `cad_transpile::tests::strip_code_fence_handles_fenced_and_bare_replies` and
  `strip_code_fence_removes_leading_and_trailing_prose_around_ecky_model`.

## 3. Dev CLI (optional harness — model/provider comparison)

- [x] 3.1 Bin `cad_to_ecky <input> [--backend] [--model] [--base-url] [--out]`.
  Evidence:
  `src-tauri/src/bin/cad_to_ecky.rs` parses every named argument;
  `cad_to_ecky_cli::{cli_flags_then_nvidia_then_nim_override_selected_engine_per_field,dump_prompt_respects_backend_flag,dump_prompt_rejects_invalid_arguments}`
  pins flag precedence, backend parsing, and invalid usage.
- [x] 3.2 `--dump-prompt` prints the assembled system+user with no API call
  (free inspection / diffing across models).
- [x] 3.3 (test) arg parsing + `--dump-prompt` output is deterministic and
  network-free.

## 4. Source adapters (extraction only; never emit Ecky)

- [x] 4.1 OpenSCAD `.scad`: pass-through text. (test) round-trips unchanged into
  the user message. Evidence: `cad_transpile::tests::openscad_adapter_preserves_source_bytes_in_the_transpile_user_message`
  pins CRLF, Unicode, comments, and trailing newline bytes through the adapter
  into `build_transpile_messages`.
- [x] 4.2 FreeCAD `.fcstd`: a fresh freecadcmd extractor that dumps the feature
  tree to JSON (extraction only — no Ecky emission). The JSON is the source text.
  Evidence: `cad_transpile::tests::fcstd_adapter_uses_fresh_freecadcmd_feature_tree_json_without_emitting_ecky`
  passes with fixture JSON and a fake command runner; no installed FreeCAD is required.
- [x] 4.3 STEP/BREP `.step`: textual summary (bodies, dims, key features) as
  source text. Faithfulness is then enforced by the parity gate, not the adapter.
  Evidence: `cad_transpile::tests::{step_adapter_summarizes_bodies_dimensions_and_features_without_ecky,brep_adapter_summarizes_topology_and_coordinate_extent_without_ecky}`
  pass against fixtures and prove no Ecky emission.
- [x] 4.4 Adapter dispatch by extension/sniff; unknown → treat as raw text.
  Evidence: `cad_to_ecky_cli::{dump_prompt_passes_unknown_extension_raw_text_verbatim,dump_prompt_preserves_unknown_extension_raw_decode_error}` proves an unknown extension carries CRLF/Unicode/trailing-newline source unchanged into `--dump-prompt`, without a fence, and preserves the raw `read input` UTF-8 error rather than using the OpenSCAD adapter.

## 5. Verify gate + repair loop

Checked items below record prior transpile behavior. Lossless-version-history
migration remains explicit in §5.5–§5.7.

### V1 (UI / consumer) — model + dialogue verify
- [x] 5.1 (prior behavior) After transpile, compile + render + `verify_generated_model` (structural
  + model-authored `(verify …)`) as normal; never auto-commit a red result.
  Evidence: `cad_transpile::tests::gate_compiles_renders_then_runs_existing_generated_model_verification`
  pins stage order and commit readiness; capped-red coverage pins false readiness.
- [x] 5.2 Dialogue-accrued verify: when the user states a requirement ("ears
  separate"), the model adds the matching `(verify …)` clause **with** the
  geometry change, so it persists as a check. Size/intent errors that pass
  structural checks are caught by the human in the loop, not by source parity.
  Evidence: `cad_transpile::tests::dialogue_requirements_accumulate_and_demand_matching_verify_clauses`
  and `dialogue_requirement_without_executed_authored_check_stays_red`.
- [x] 5.3 (prior behavior) Repair loop: feed the compiler/verify diagnostic back to the model (per
  the API operating contract) and re-request, capped; report capped red honestly
  without commit. Evidence:
  `cad_transpile::tests::compiler_diagnostic_drives_bounded_repair_then_green_verification`
  and `capped_red_verification_reports_diagnostic_and_never_becomes_commit_ready`.

### Internal CLI only (proving-ground, NOT the UI)
- [x] 5.4 Source parity: where the source is measurable, compare source bbox+volume
  (source → STEP/STL → `import_step`) to the rendered Ecky within tolerance;
  surface a mismatch as a parity diagnostic. Auto-catches size errors (the 2×
  head). Source-specific runtimes (freecadcmd, STL/STEP measurer) live behind the
  CLI; used to vet components before release. Do NOT wire into the UI. Evidence:
  `cad_transpile::tests::measurable_source_bbox_and_volume_mismatch_fail_internal_parity_gate`
  and `ui_tier_without_source_measurement_skips_parity`.

- [ ] 5.5 (BDD red) Append each distinct emitted/draft source before compile,
  render, or verify; prove invalid/pending output becomes head and unchanged
  observations do not duplicate versions.
- [ ] 5.6 (BDD red) Replace conflict/thread-advanced/force refusal with serialized
  append semantics; prove stale/concurrent emissions persist, last append is
  head, and late diagnostics attach to their originating version.
- [ ] 5.7 (BDD red) Store raw diagnostics on appended versions; add explicit
  successful/shippable filter/count separate from head/history. Repair outputs
  append new versions.

## 6. Surfaces

- [x] 6.1 Code window (V1 only): `translate to Ecky` toggle/action sends the
  current buffer as the source and replaces it with the result; keep the original
  recoverable on failure (no silent clobber of a good buffer). No FreeCAD or source
  parity in the UI. Evidence: `e2e/cad-transpile-code-window.spec.ts` proves the
  real-route happy, pending, and raw-provider-error flows; `cadTranspile.test.ts`
  pins verbatim source carriage into the existing authoring prompt.
- [x] 6.2 MCP: document the message convention (send foreign code + ask to
  transpile → agent writes a new thread version). Optionally add a thin
  `cad_transpile` verb that canonicalises the translate instruction; it must be
  pure sugar over the existing authoring flow. Evidence: authored convention in
  `skills/ecky-mcp/SKILL.md`; no new MCP tool or alternate authoring path added.

## 7. Optional: stdlib seeding consumer

- [x] 7.1 A transpiled + curated + verified model may go through
  `component_extract --save` like any other; no bespoke path. Stdlib's source of
  truth stays the hand-authored curated set (`language-convenience-stdlib`).
  Evidence: existing
  `mcp::server::tests::component_extract_tool_handler_extracts_and_saves_to_library`
  proves ordinary Ecky source extracts, saves, searches, and reads back through
  the shared component library path; transpile adds no component consumer.

## Migration note

The deterministic-emitter prototype and the library-survey script
(`scripts/freecad-transpiler/`) have been removed — their concept is dead. The
kill evidence (0% Array/expression/spreadsheet, 45% PartDesign over 97 mechanical
parts) is recorded in the proposal. The FreeCAD adapter (task 4.2) is built fresh
as extraction-only when needed; the LLM owns all Ecky emission.

## 8. Remove the retired Thomas Python converter — 2026-10-12

- [x] Reproduce the registered obsolete CLI target with a failing Cargo metadata check.
- [x] Delete the Thomas-only Python AST emitter, its module export and tooling CLI target.
- [x] Delete the Python reference macro and both sets of subprocess parity scripts;
  update the aggregate parity list and current documentation references.
- [x] Prove that Cargo metadata retains `cad_to_ecky` and omits the retired target.
- [x] Run current backend and transpile CLI acceptance suites.

Current `.ecky` ramp fixtures and their compiler/FreeCAD regression tests remain
active. The removed parity test no longer launches Cargo from inside a test or
adds a runtime path for an old authored format.

Retirement evidence: Cargo metadata absence check first failed on the registered
old target, then passed after removal. All 48 backend tests and 13 current
transpile CLI tests pass. The aggregate FreeCAD parity runner references eight
existing scripts. No code, tooling script, or current tracker references the
removed module, macro, or Thomas Python parity runners.
