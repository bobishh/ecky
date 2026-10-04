# Change: Global Experimental Jev Routing

## Why

Jev is an application-wide request classifier and model router. The current
implementation exposes the setting only for Codex, dispatches classification only
from the Codex adapter, and makes a trusted Codex PreToolUse hook a prerequisite.
An enabled global setting therefore fails a Codex request and has no effect on
Agy or API engines. This contradicts the requested provider-independent behavior.

Request routing and provider-native tool interception are separate concerns.
Jev selects a typed request contract; Rust owns execution and deterministic policy.
A provider hook must not determine whether the classifier can run.

## What Changes

- Keep one off-by-default `jevClassifier` setting and one masked TypeSafe token,
  persisted through canonical config EDN. Show them in common agent settings for
  every connection mode, including API, Codex, Agy, and managed MCP agents.
- Add one shared Rust routing service used by every application-owned user-request
  dispatch. Cover every configured API engine through its common API path, owned
  Codex and Agy sessions, and Ecky's managed MCP request-delivery path.
- Classify bounded current request/context once per accepted dispatch attempt.
  Compose Answer, Plan, Clarify, Inspect, Modify, and answer-plus-action contracts.
  No keyword classifiers, provider-specific intent rules, or frontend orchestration.
  Preserve selected action under moderate confidence; ask only when a strongly
  indicated missing fact blocks action. Expose accepted action probabilities through
  a separate request/message result, without adding fields to message payloads.
- Apply the selected prompt and existing Rust-controlled capability policy through
  adapter boundaries. Use the actual provider for user-facing answers: Jev returns
  judgments, not generated prose or a replacement authoring model.
- Select only eligible models within that adapter's configured ceiling when model
  selection and comparable prices are established. Unknown prices, provider-default
  settings, or caller-owned model selection preserve the model and still classify
  intent. Never switch provider accounts or invent subscription cost comparisons.
- Remove automatic startup-hook registration and hook-trust gating from ordinary
  Jev routing. Retained native enforcement may run only as a separately configured
  adapter capability; it must not be silently activated by `jevClassifier.enabled`.
- Preserve existing Rust queue/controller ownership, immutable versions, cancellation,
  retries, history, and file-backed route evidence across all adapters.

## Status and Scope

This is the revised contract requested on 2026-09-29. Global routing is implemented
across the application-owned adapters; remaining unchecked tasks track cross-adapter
proof, calibration, and cancellation coverage. Existing Codex-only work and installed
native-hook probes remain historical baseline evidence in `implementation-evidence.md`,
`codex-boundary-notes.md`, and `trace-audit.md`; they do not prove universal provider-native
tool interception.

All application-owned user requests are in scope. External agents calling MCP
outside an Ecky-owned request cannot have their private model/session controlled
by Ecky; the Rust MCP boundary still enforces applicable bound-request policy.
Jev is never a per-tool authorization call. Existing exploration and immutable
version services remain authoritative.

## Out of Scope

- Default-on activation, automatic token migration, or a new frontend lifecycle.
- New conversations, schedulers, queues, or a separate eval database.
- Blanket hook trust, trust-database writes, or automatic permission escalation.
- Claiming interception of native shell/file tools without adapter-specific proof.
- Inferring model price or capability from names, switching providers automatically,
  or rewriting user model preferences.
- New per-action approval UI, native-hook setup UI, or per-tool Jev assessment.

## Impact

- Common settings: `src/lib/ConfigPanel.svelte`; existing JSON/EDN config contract.
- Shared routing: Rust service, existing `jev_classifier.rs`, `provider_turn.rs`.
- API request routing/generation, Codex dispatch, Agy dispatch, managed MCP delivery.
- Existing provider-neutral `llm_eval.rs` route evidence.
- Synchronizes `exploration-build-cycle`; preserves existing adapter ownership.

## Proof Plan

Outer BDD first: global Settings visibility/save/reload and missing-token failure;
then real Rust adapter integration fixtures for each dispatch family. Confirm red
failures before implementation. Prove classifier consumption, model preservation,
no mandatory hooks, failed classification without execution, disabled zero-Jev,
cancellation/staleness, and reuse across transport/build retries. Unit tests drive
shared validation and composition. Run relevant Rust tests as compile proof,
Playwright happy plus failure/pending flows, and strict OpenSpec validation.
Live account quality/cost calibration remains separately reported work.
