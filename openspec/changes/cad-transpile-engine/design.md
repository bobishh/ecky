# Design: CAD Transpile (thin, over the existing agent/LLM path)

## 1. Principle

Transpile = a normal LLM request with a translate intent. No new engine, no new
client, no new prompt. The only invariants:

- **system prompt** = `agent_prompt::agent_language_reference(backend)` (verbatim,
  already shared by MCP + API mode);
- **user content** = a short translate instruction + the foreign source;
- **output** = `.ecky`, fed straight into the existing compile + `verify` gate;
- **trust** = only after render + (where measurable) geometry parity to source.

## 2. Pipeline

```
foreign source (OpenSCAD / FreeCAD-JSON / STEP-summary / text)
   └─[adapter: text only if binary]→ source text
        └─[request: system=agent_language_reference, user=translate+source]→ LLM
             └─ .ecky → compile → verify_generated_model → (parity to source)
                  └─ green → buffer / new thread version ; red → diagnostic → repair
```

Everything after "→ LLM" is the existing path.

## 3. Surfaces

### 3.1 Code window (UI) — V1 only
A `translate to Ecky` toggle/action in the code panel. On invoke: take the
current buffer as the source, run the request, replace the buffer with the
returned `.ecky`, and let the normal render/verify run. Failure shows the
compiler/verify diagnostic in place (no silent overwrite of a good buffer — guard
behind a confirm or keep the original recoverable). The UI ships **V1**: LLM
transpile + model/dialogue `(verify …)`. **No FreeCAD dependency and no source
parity in the UI** — that arsenal stays in the CLI (§3.3, §6).

### 3.2 MCP
No new tool required: a user sends a thread message containing the foreign code
and asks to transpile. The in-app agent (whose system prompt is already
`agent_language_reference`) reads the message and authors a **new thread version**
via `macro_preview_render` / `commit_preview_version` — identical to any other
authoring turn. Optionally a thin `cad_transpile` MCP verb can canonicalise the
translate instruction, but it is sugar over the same message flow.

### 3.3 Internal CLI (our proving-ground, not a consumer surface)
A bin (`cad_to_ecky`) we use to compare models/providers and to **vet components
before release**: `cad_to_ecky <input> [--backend …] [--model …] [--base-url …]
[--dump-prompt] [--out …]`. It assembles the same system+user messages and posts
to an OpenAI-compatible endpoint (NIM default). `--dump-prompt` prints the
assembled prompt with no API call. The heavy source-parity arsenal (§6) hangs off
this CLI, never the UI. This is the "military" tier — full power, for us; the
consumer gets only its vetted output.

## 4. Reused building blocks (do not rebuild)

| Need | Existing |
| --- | --- |
| Ecky system prompt | `agent_prompt::agent_language_reference(backend)` |
| Provider/model/key/base_url | `Config { provider, api_key, base_url, model }` (NIM = OpenAI-compatible `base_url`) |
| Chat request + parse | `llm.rs` (`send_openai_request`, `extract_openai_message_content`) |
| Thread version authoring | `macro_preview_render`, `commit_preview_version` |
| Gate | `verify_generated_model` + authored `(verify …)` |
| Geometry parity measurement | source → STEP → build123d `import_step` bbox/volume |
| CLI shape precedent | `src/bin/translate_legacy_python_to_ecky_ir.rs` |

## 5. The translate instruction (the only new prompt text)

A short, fixed preamble prepended to the source in the user message, e.g.:

> Translate the following CAD source into a single parametric Ecky `(model …)`.
> Infer meaningful parameters (sizes, counts, repeats) rather than copying dead
> numbers; emit loops (`repeat-union` / `for-union`) for repeated features. Output
> only Ecky source.

It carries the **semantic** ask (parametrize, loop-ify) the deterministic walker
could not do. The geometry rules and op catalogue come from the system prompt.

## 6. Verify gate — tiered

**V1 (UI / consumer):**
1. Compile + render + `verify_generated_model` (structural + model/dialogue
   authored `(verify …)` clauses) — already the standard gate.
2. On red: return the diagnostic to the model (the API operating contract already
   says "treat the compiler diagnostic as authoritative; fix the named cause and
   re-emit") and re-request, capped. Never auto-commit red.
3. Size/intent errors that pass structural checks (e.g. a 2× hex head — manifold,
   single solid, but wrong size) are caught by the **human in the loop**: the user
   says "head's too big", the model fixes it and adds a clause pinning the
   dimension. No source measurement.

**Internal CLI only (our proving-ground, not the UI):**
4. Where the source is measurable, measure source bbox+volume (source → STEP/STL →
   `import_step`) and compare to the rendered Ecky within tolerance — this
   auto-catches the 2× head with no human. Needs source-specific runtimes
   (freecadcmd, an STL/STEP measurer), so it hangs off the CLI and is used to vet
   components before they ship. Not wired into the UI.

## 7. Adapters (only source-specific code; extraction, never emission)

- **OpenSCAD** (`.scad`): pass the text through; the model reads it directly.
- **FreeCAD** (`.fcstd`): a fresh freecadcmd extractor dumps the feature tree to
  JSON (extraction only). The JSON is the source text.
- **STEP/BREP** (`.step`): a textual summary (bodies, dims, key features) as the
  source text; geometry parity then guards faithfulness.
- **Raw/pseudo-CAD text**: pass through.

Adapters produce text for the model; they never emit Ecky.

## 8. Removed prototype

The deterministic FreeCAD emitter and library-survey scripts have been deleted —
their concept (mechanical AST mapping + bulk seeding) is dead per the proposal's
evidence. The FreeCAD adapter is built fresh as extraction-only when needed.

## 9. Alternatives considered

- **Deterministic AST transpiler** (the removed design): rejected on evidence —
  no Array/expression parametricity in the real library, so output is dead
  numbers needing manual re-parametrization; PartDesign unhandled.
- **A standalone transpile engine/service**: rejected — the app already has the
  LLM path, system prompt, version writing, and gate. Transpile is a thin
  intent over them, not a subsystem.
- **Trusting LLM output without parity**: rejected — silent coordinate
  hallucination. The gate is mandatory.
