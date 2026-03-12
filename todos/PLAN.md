# MCP Phase 2: Bounded Agent Sessions for Generated Models

## Summary
Phase 2 turns the current `freecad-mcp` from a draft/save toolset into a bounded multi-step workflow for external agents. It stays `generated-models only`, keeps the existing provider/API-key flow unchanged, and keeps `freecad-mcp` as a tool server without its own internal LLM.

Chosen defaults for Phase 2:
- focus: stronger agent workflow, not visual/import scope
- target scope: generated macro-driven models only
- agent mode: bounded autopilot contract for the external host
- save policy: auto-save every successful step into the same thread
- explicit fork remains available via `thread_fork_from_target`

## Key Changes
### 1. Durable agent-run layer
Add persistent run state on top of the existing transient `agent_sessions` table.

New tables:
- `agent_runs`
  - `run_id`, `client_kind`, `agent_label`, `goal`, `label`
  - `thread_id`, `base_message_id`, `current_message_id`, `current_model_id`
  - `max_steps`, `steps_used`
  - `status` (`active`, `waiting`, `completed`, `cancelled`, `error`)
  - `last_error`, `created_at`, `updated_at`
- `agent_run_steps`
  - `run_id`, `step_index`, `tool_name`
  - `step_label`
  - `source_message_id`, `saved_message_id`, `model_id`
  - `status` (`success`, `error`, `cancelled`)
  - `error_text`, `created_at`

Rules:
- `agent_sessions` remains the live activity table for badges/overlays.
- `agent_runs` is the durable source of truth for workflow progress and auto-saved history grouping.
- `steps_used` increments on every mutation attempt under a run, including failed ones.
- when `steps_used == max_steps`, the run becomes `completed` after a success, or `error` after a failure.
- successful session-mode mutations do not leave a persistent draft behind; they render and immediately save a version.

### 2. MCP workflow contract
Close the missing context gap from Phase 1 and add a bounded session API.

Add tools:
- `thread_get`
  - input: `threadId`
  - output: full thread with messages, `artifactBundle`, `modelManifest`
- `agent_session_start`
  - input: optional `threadId`, optional `messageId`, `goal`, optional `label`, optional `maxSteps`
  - defaults: `label = "Agent"`, `maxSteps = 8`
  - output: `runId`, resolved target ids, `maxSteps`, `stepsUsed = 0`, `status = active`
- `agent_session_get`
  - input: `runId`
  - output: run state, current target ids, last error, step list
- `agent_session_finish`
  - input: `runId`, optional `summary`
  - output: run state with `status = completed`
- `agent_session_cancel`
  - input: `runId`
  - output: run state with `status = cancelled`
- `version_compare`
  - input: `leftMessageId`, `rightMessageId`
  - output: thread ids, model ids, parameter diff, macro diff summary, optional unified macro diff, preview asset paths

Extend existing mutation tools:
- `params_patch_and_render`
- `macro_replace_and_render`

New optional inputs on both:
- `sessionId`
- `stepLabel`

New session-mode behavior:
- resolve target from `session.current_message_id`, not from loose `threadId/messageId`
- validate the run is `active` or `waiting`
- increment `steps_used`
- render
- on success:
  - auto-save a new version into the same thread
  - create `agent_run_steps` row with `status = success`
  - advance `current_message_id` and `current_model_id`
  - clear any target draft
  - return the new saved `messageId` and `stepIndex`
- on failure:
  - do not save a version
  - create `agent_run_steps` row with `status = error`
  - set run `status = waiting` if budget remains, otherwise `error`
  - return the raw backend/render error

Auto-saved version naming:
- if `stepLabel` is present: `{label} {stepIndex:02d} - {stepLabel}`
- otherwise: `{label} {stepIndex:02d}`

Saved message content:
- standardized as `Agent session step {stepIndex} committed automatically.`

### 3. Parameter and contract ergonomics
Keep the boundary strict and remove the current ambiguity around parameter keys.

Rules:
- top-level MCP arguments stay `camelCase`
- keys inside `initialParams`, `parameterPatch`, and other model param maps remain model-native keys exactly as stored in the macro/ui schema
- for generated models that means `snake_case`; there is no inner auto-translation

Validation upgrades:
- unknown patch keys must fail with a validation error that includes the offending key and up to 3 close matches from `uiSpec`
- invalid select values must report the field key and the allowed option values
- `macro_replace_and_render` must keep accepting explicit `uiSpec` for legacy macros and must validate `initialParams` against the effective `uiSpec` before render

### 4. Desktop integration
Expose durable run state in the app and make auto-saved runs readable rather than noisy.

Backend read commands:
- `get_external_agent_runs`
- `get_external_agent_run_steps`
- `compare_versions`

Frontend behavior:
- history groups auto-saved versions by `runId`
- grouped run shows label, goal summary, step count, current status, last error
- compare action is available between base vs latest and any two saved steps in the same run
- viewer follows the newest external saved step only when the current target matches and the local working copy is clean
- if local working copy is dirty, do not overwrite; show `agent run advanced`
- show `cancel run` action in the desktop UI by calling the new backend cancel command

### 5. Minimal MCP prompts/resources
Add only the workflow scaffolding needed for bounded autopilot hosts.

Add:
- one prompt template for `run a bounded editing session`
- one resource for `current run context`

They should describe:
- current goal
- current saved target
- remaining step budget
- required tool order
- stop conditions

This is not a general resources/prompts expansion; it is only the minimum needed to make multi-step host behavior consistent.

## Test Plan
Rust/backend:
- `agent_session_start` resolves explicit `messageId`, explicit `threadId`, and current snapshot correctly
- session-mode `params_patch_and_render` auto-saves a new version and advances `current_message_id`
- session-mode `macro_replace_and_render` auto-saves and preserves explicit legacy `uiSpec`
- failed session mutation creates no saved version and records `waiting`/`error` correctly
- step budget is enforced exactly
- `version_compare` returns correct param and macro diffs
- unknown parameter keys return close-match suggestions
- generated-only guard rejects imported-model targets for session mode

Frontend:
- grouped run UI appears on thread history
- compare action works for base vs latest and step vs step
- clean local state follows latest saved external step
- dirty local state is not overwritten
- cancel button updates visible run state

Verification:
- `cd /Users/bogdan/Workspace/personal/alcoholics_audacious/ecky/src-tauri && cargo check`
- relevant Rust tests for run lifecycle and auto-save behavior
- `npm run typecheck`
- frontend tests for run grouping, compare, and hydration guards
- one manual smoke test from an MCP host: `session_start -> macro_replace_and_render -> params_patch_and_render -> version_compare -> session_finish`

## Assumptions
- `freecad-mcp` still does not embed its own LLM; “autopilot” is a bounded workflow contract consumed by Gemini/Claude/Codex
- Phase 2 does not add imported `FCStd` editing, screenshot reasoning, or part-level highlight
- same-thread auto-save is the default and only Phase 2 policy; forking stays explicit
- existing non-session draft flow from Phase 1 remains supported for manual host chaining
