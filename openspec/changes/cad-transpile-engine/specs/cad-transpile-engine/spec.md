# Delta for cad-transpile-engine

## ADDED Requirements

### Requirement: Foreign CAD source is transpiled to parametric Ecky via the LLM path

The system SHALL translate foreign CAD source (e.g. OpenSCAD, a FreeCAD feature
tree, a STEP summary, or raw pseudo-CAD text) into a single parametric Ecky
`(model …)` by issuing an LLM request whose system prompt is the existing
single-source Ecky language reference (`agent_language_reference`) and whose user
content is a translate instruction plus the source. No bespoke per-language AST
transpiler is introduced; source-specific code is limited to text-extraction
adapters that never emit Ecky.

#### Scenario: OpenSCAD source becomes parametric Ecky

- GIVEN an OpenSCAD source with a repeated feature and named dimensions
- WHEN it is transpiled
- THEN a single `(model (params …) (part …))` is emitted
- AND repeated features are expressed as loops (`repeat-union`/`for-union`), not
  hardcoded copies
- AND it compiles on the active backend.

#### Scenario: System prompt is the shared language reference, not a bespoke one

- GIVEN any supported backend
- WHEN the transpile request is assembled
- THEN its system prompt equals `agent_language_reference(backend)`
- AND the user message contains the translate instruction and the source verbatim.

### Requirement: Transpiled output is gated, tiered by surface

The system SHALL treat LLM output as untrusted until it compiles and passes
verification. In the UI / consumer tier (V1), verification is compile + render +
the model- and dialogue-authored `(verify …)` clauses, with the human in the loop
catching size/intent errors; the UI SHALL NOT depend on FreeCAD or on source
measurement. Automated source-parity (bounding box + volume against a measurable
source) is internal CLI tooling for pre-release vetting and SHALL NOT be wired
into the UI. Red results SHALL NOT be auto-committed or shipped in either tier;
the diagnostic SHALL be returned for a capped repair loop.

#### Scenario: V1 catches a requirement via a dialogue-authored clause

- GIVEN a transpiled model and a user instruction stating a requirement (e.g. "the
  ears should be separate")
- WHEN the model applies the change
- THEN it adds a matching `(verify …)` clause (e.g. component count) together with
  the geometry change
- AND that clause persists and runs on subsequent versions.

#### Scenario: Source parity is an internal CLI gate, not a UI feature

- GIVEN a measurable source (e.g. FreeCAD or STEP) processed in the internal CLI
- WHEN the transpiled Ecky renders but its bounding box/volume diverge from the
  source beyond tolerance
- THEN the CLI reports a parity failure to drive a repair re-request
- AND this source-measurement path is absent from the UI tier.

#### Scenario: Failed compile drives repair, not silent acceptance

- GIVEN transpiled output that fails to compile
- WHEN the gate runs
- THEN the compiler diagnostic is returned to the model for re-emission
- AND no version is committed while the result is red.

### Requirement: Model and provider are configurable, defaulting to the app config

The system SHALL resolve the LLM provider, model, API key, and base URL from the
app `Config` first and environment overrides second, using the existing
OpenAI-compatible client. An OpenAI-compatible endpoint (e.g. NVIDIA NIM) SHALL be
usable with no new client code.

#### Scenario: NVIDIA NIM token from config drives the transpile

- GIVEN an app `Config` (or env) with an OpenAI-compatible `base_url`, `api_key`,
  and `model` (e.g. a NIM endpoint)
- WHEN a transpile is requested
- THEN the request is sent through the existing OpenAI-compatible path to that
  endpoint
- AND swapping the configured model changes which model performs the transpile.

### Requirement: Transpile is reachable from the code window and from MCP

The system SHALL expose transpile through the code window (a `translate to Ecky`
action on the current buffer) and through MCP (a thread message carrying foreign
code with a transpile request, answered by a new Ecky thread version), both
reusing the existing authoring and gate path.

#### Scenario: Code-window toggle translates the current buffer

- GIVEN foreign source in the code buffer
- WHEN the user invokes `translate to Ecky`
- THEN the buffer is sent as the source and replaced with the transpiled Ecky
- AND the normal render/verify runs
- AND on failure the original buffer remains recoverable (no silent clobber).

#### Scenario: MCP message transpiles into a new thread version

- GIVEN a thread and a message containing foreign CAD code with a transpile ask
- WHEN the agent processes it
- THEN it authors a new thread version in Ecky via the existing version path
- AND that version is subject to the standard verify gate.
