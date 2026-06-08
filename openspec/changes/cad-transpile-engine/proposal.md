# Proposal: CAD Transpile — Any Source → Parametric Ecky (LLM, thin)

## Intent

Let a user turn **foreign CAD source** (OpenSCAD, a FreeCAD feature tree, a STEP
summary, a stray sketch — anything a model can read as text) into **parametric
Ecky**, by reusing the agent/LLM path the app already has. Transpile is **not a
new engine**: it is an ordinary translate-intent request whose system prompt is
the existing single-source Ecky language reference and whose user content is the
foreign source. The model emits `.ecky`; the existing compile + `verify` path
gates it.

Two surfaces, one mechanism:

- **Code window** — a `translate to Ecky` toggle/action: the current buffer is
  sent as a translate request; the returned `.ecky` replaces the buffer and
  compiles/verifies normally.
- **MCP** — send a thread message containing the foreign code and ask to
  transpile; the agent reads the message and writes a **new thread version** in
  Ecky (the same way it writes any other version).

## Why this replaces the deterministic FreeCAD transpiler

The earlier change here was a deterministic FreeCAD-feature-tree → Ecky AST
transpiler. We killed it on evidence. Over a 97-part spread of the FreeCAD
library (of 2498 mechanical parts):

- 45% are PartDesign (Pad/Pocket/Sketch) — the deterministic walker did not
  handle them;
- of the 54% Part-workbench parts, **0% carried a `Part::Array`, 0% an
  expression binding, 0% a Spreadsheet**.

So the promised "parametric for free" (Array→loop, expression→param) **does not
exist** in the real library: the parts are sketch+extrude with hardcoded
numbers. A deterministic transpile of that yields dead-number Ecky that a human
must re-parametrize anyway. **Parametrization is a semantic judgement, not a
mechanical mapping** — recognising "this is an M8 bolt → parametrize by M", or
"these four holes → a `repeat-union` grid" — which is exactly what an LLM does
well and a deterministic walker cannot.

The roles therefore invert: an LLM owns the semantic translation +
parametrization; deterministic code owns only what code is good at —
**verifying** the result (render + geometry parity) so a hallucinated coordinate
cannot ship silently.

## Why it is thin (reuse, not build)

Everything the transpile needs already exists in the app:

- **System prompt**: `agent_prompt::agent_language_reference(backend)` — the
  self-contained, drift-free Ecky reference already used by both the in-app MCP
  agent and API mode. The transpile uses it verbatim.
- **LLM path + model/provider config**: `Config { provider, api_key, base_url,
  model }` and the OpenAI-compatible client in `llm.rs`. NVIDIA NIM is an
  OpenAI-compatible `base_url`, so a free/cheap NIM token works with no new
  client — and a free tier means retries (the verify-repair loop) cost nothing,
  so a weaker model is acceptable.
- **Version writing**: the agent already authors thread versions
  (`macro_preview_render` / `commit_preview_version`).
- **Gate**: `verify_generated_model` + authored `(verify …)` already run on every
  version.

The only genuinely new pieces are the two thin affordances (a code-window toggle;
an MCP transpile convention) and an optional dev CLI for comparing models.

## Source-agnostic by construction

Because the source is just text handed to the model, the input language is open:
OpenSCAD, FreeCAD feature-tree JSON, a STEP/BREP textual summary, or hand-written
pseudo-CAD. No per-language parser is required for the model to read it. Where a
binary source needs reducing to text first (a `.fcstd` or `.step`), a small
adapter produces that text (e.g. freecadcmd dumps the feature tree to JSON); the
adapter is the only source-specific code, and it does extraction, never emission.

## Tiered verification

LLMs get geometry right structurally but make quiet numeric errors (an observed
transpile produced a hex head 2× oversized — manifold and single-solid, so a
structural check passes, but the wrong size). Verification is therefore tiered by
product, not bolted on as one mandatory step:

- **V1 — model + dialogue verification (the UI / consumer surface).** The model
  authors `(verify …)` clauses for what it claims, and the conversation accrues
  more: when the user says "the ears should be separate", the model adds the
  matching clause (e.g. `stl connected-component-count = 2`, or a `clearance`
  minimum) **together with** the geometry change, so the requirement becomes a
  persistent check. The gate is compile + render + these authored clauses
  (`verify_generated_model`, already in the app). The human in the loop catches
  size/intent errors — the 2× head is caught by the user saying "head's too big",
  not by source measurement. **No source-specific tooling, and no FreeCAD in the
  UI** — the UI ships V1 only.
- **Source parity — internal tooling, not the UI.** Measuring the *source*
  (FreeCAD `.fcstd` → freecadcmd → STEP; STL/STEP directly) and comparing
  bbox+volume to the rendered Ecky within tolerance. This auto-catches the 2× head
  with no human, but needs source-specific runtimes (freecadcmd, an STL/STEP
  measurer) and only works for measurable sources. It lives in **our internal CLI
  proving-ground** (the arsenal we use to vet components before release), not in
  the consumer UI. If ever exposed to users it is a premium surface, but its
  primary role is our pre-release QA.

The architectural consequence is simply: **do not wire FreeCAD into the UI; the UI
does V1.** The heavy parity arsenal stays in the CLI. Either way, un-verified
output is never silently shipped as a stdlib component — the difference is whether
the catch is human (V1) or automated (internal parity).

## Consumers

- **Primary**: user-facing "bring foreign CAD into Ecky as editable parametric
  source" — per-use value, on demand.
- **Optional**: seeding stdlib components — a transpiled, curated, verified model
  can still go through `component_extract --save` like any other. But stdlib
  itself is hand-authored from a curated set (see `language-convenience-stdlib`);
  transpile is a convenience, not the stdlib's source of truth.

## Out of scope

- A bespoke deterministic AST transpiler for any source language (the thing we
  removed). Source-specific code is limited to text-extraction adapters.
- FreeCAD or auto source-parity in the UI. The UI ships V1 (LLM transpile +
  model/dialogue verify) with no FreeCAD dependency; bbox/volume source-parity is
  internal CLI tooling for pre-release vetting, not a consumer UI surface.
- Shipping a curated stdlib from bulk transpile. The library is family-redundant
  (1034 fastener files are size-variants of ~5 parametric families); the win is a
  few hand-authored parametric components, not hundreds of transpiled ones.
