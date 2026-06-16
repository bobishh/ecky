# agent-engine-config Specification

## Purpose

Defines how the user configures LLM engines (providers, API keys, models) and how
Ecky decides whether an engine/model can consume image inputs. Covers the
single-active-engine contract, per-model vision-capability resolution, and the
gating of image-bearing UI affordances (screenshots, attachments, drawing).

## Scope

- Settings → Agents panel: API ENGINES list, engine detail fields.
- Vision capability: override storage, name-pattern inference, optimistic default,
  and automatic disable-on-provider-rejection.
- UI gating downstream of vision capability: DRAW dock button, ATTACH REFERENCE
  file dialog, drag-drop of images.

Out of scope: MCP agent configuration, FreeCAD/runtime capability detection,
generation/verify pipeline internals beyond the vision-input decision.

## Requirements

### Requirement: Single active engine

The system SHALL treat exactly one engine as live at any time. The selected
engine (`config.selectedEngineId`) is the sole engine permitted to make API
calls; all other engines are inert.

#### Scenario: Selecting an engine makes it the sole live engine

- GIVEN two configured engines A and B, with A selected
- WHEN the user selects engine B
- THEN B's `enabled` flag is set to `true`
- AND A's `enabled` flag is set to `false`
- AND `config.selectedEngineId` is set to B's id

#### Scenario: Adding an engine promotes it to active

- GIVEN one configured engine A that is selected
- WHEN the user adds a new engine B
- THEN B is selected and `enabled = true`
- AND A's `enabled` flag is set to `false`

#### Scenario: Removing the active engine selects the first remaining

- GIVEN engines A (selected/enabled) and B
- WHEN the user removes engine A
- THEN engine B becomes selected and `enabled = true`

#### Scenario: Legacy multi-enabled configs normalize on load

- GIVEN a saved config where two engines both have `enabled = true`
- WHEN the config is loaded at startup
- THEN the selected engine keeps `enabled = true`
- AND every other engine is set to `enabled = false`
- AND the normalized config is persisted

### Requirement: Engine detail fields

The system SHALL expose, per engine: display name, provider, API key, heavy
model, light model, and an optional base URL. A per-model vision-capability
override control SHALL appear alongside the heavy-model field.

#### Scenario: Provider dropdown restricts backend routing

- GIVEN an engine detail view
- WHEN the user picks provider `gemini`
- THEN requests use the native Gemini API
- WHEN the user picks provider `openai` or `ollama`
- THEN requests use the OpenAI-compatible chat-completions path

#### Scenario: Vision override cycles through three states

- GIVEN an engine with a heavy model selected
- WHEN the user clicks the vision toggle
- THEN the override cycles `Auto → Vision → TextOnly → Auto`
- AND when `Auto`, the toggle shows the inferred capability in its tooltip

### Requirement: Vision capability resolution

The system SHALL resolve whether the selected engine's current model can consume
image inputs using this precedence: (1) a per-(engine, model) user override;
(2) model-name-pattern inference; (3) an optimistic default of vision-capable.

#### Scenario: Override is authoritative over inference

- GIVEN an engine whose model name matches a vision-positive pattern
- AND a `TextOnly` override stored for that model
- WHEN vision capability is resolved
- THEN it resolves to text-only regardless of the name pattern

#### Scenario: Name-pattern inference identifies known families

- GIVEN a model with no override
- WHEN the model id matches a vision-positive pattern (`gpt-4o`, `claude`, `gemini`,
  `multimodal`, `vl`, or a GLM vision variant like `glm-5v-turbo`)
- THEN vision capability resolves to vision-capable
- WHEN the model id matches a text-positive pattern (`instruct`, `coder`,
  `glm-[0-9]` without a vision marker, deepseek non-vl)
- THEN vision capability resolves to text-only

#### Scenario: Unknown model names default to vision-capable

- GIVEN a model with no override whose name matches no pattern
- WHEN vision capability is resolved
- THEN it resolves to vision-capable

### Requirement: Automatic disable on provider rejection

The system SHALL detect provider errors indicating the model rejects image inputs
and, on the first such failure for a request, persist a `TextOnly` override for
that (engine, model), drop all image inputs, and retry the attempt once.

#### Scenario: Text-only rejection triggers a self-healing retry

- GIVEN an optimistic vision-capable model that in fact rejects images
- WHEN a generate call fails with a 400 whose body indicates `content.type` is
  invalid or only `text` is allowed
- THEN a `TextOnly` override is written to the engine's `visionOverrides` for that
  model and persisted
- AND the screenshot and image attachments are dropped
- AND the attempt is retried without images
- AND the retry is not repeated for the same request

#### Scenario: Non-vision failures are not retried as text

- GIVEN a generate call that fails for a reason other than image rejection
- WHEN the failure is handled
- THEN no vision override is written and no text-only retry occurs

### Requirement: Image-input UI gating

The system SHALL disable image-producing affordances when the resolved vision
capability is text-only, and restrict image selection in the attach dialog.

#### Scenario: DRAW dock button is disabled for text-only models

- GIVEN the selected engine resolves to text-only
- THEN the DRAW dock button is disabled
- AND its tooltip states the reason

#### Scenario: Attach dialog restricts to CAD for text-only models

- GIVEN the selected engine resolves to text-only
- WHEN the user opens the ATTACH REFERENCE dialog
- THEN only CAD/macro extensions are offered
- AND dropped image files are rejected silently

#### Scenario: Image attachments remain available for vision-capable models

- GIVEN the selected engine resolves to vision-capable
- THEN the DRAW dock button is enabled
- AND the attach dialog offers image extensions alongside CAD/macro
