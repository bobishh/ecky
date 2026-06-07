# Tasks: Agent Relay Telepathy

BDD dual-loop. Each slice: failing test first (red), minimum code (green),
refactor green. Run `npm run test:unit` after each green; targeted Playwright +
`cd src-tauri && cargo check` before the final checkpoint.

## 1. Relay detection (pure helper)

Write scope:

- new `src/lib/agents/relayPresence.ts`
- new `src/lib/agents/relayPresence.test.ts`

Tasks:

- [x] 1.1 (red) Unit test: `resolveRelayPresence` returns `null` when connection
  is not MCP, for any agent/source.
- [x] 1.2 (red) Unit test: returns `null` when the bubble source is not
  agent/thread provenance (`threadAgentActivity` | `threadAgentMascot` |
  `threadError`).
- [x] 1.3 (red) Unit test: returns `null` when the sending agent IS the primary
  agent (via `promptBelongsToPrimaryAgent`).
- [x] 1.4 (red) Unit test: returns `{ hue, label }` for an MCP, non-primary,
  agent-sourced bubble, where `hue === buildAgentGenieTraits(identity).colorHue`.
- [x] 1.5 (red) Unit test: same identity → same hue across calls.
- [x] 1.6 (green) Implement `resolveRelayPresence`; reuse `usesMcpConnection`,
  `promptBelongsToPrimaryAgent`, `buildAgentGenieTraits`.
- [x] 1.7 (refactor) De-dupe predicates, keep the function pure.

## 2. VertexGenie relay overlay

Write scope:

- `src/lib/VertexGenie.svelte`

> Note: the repo has no in-process Svelte/DOM unit harness (`test:unit` is
> `tsx --test` over pure `.ts`). The bubble attribution/`data-relay` DOM contract
> is therefore proven by the Playwright e2e in task 4, not a standalone unit test.

Tasks:

- [x] 2.1 (covered by e2e) With `relay` set, the bubble shows the sending agent's
  attribution tag (`data-relay-label`) alongside the default Ecky copy.
- [x] 2.2 (covered by e2e) With `relay` set, the bubble carries the relay marker
  (`data-relay="true"`) and a hue-tinted border (`--relay-hue`).
- [x] 2.3 (green) Add `relay: { hue: number; label: string } | null` prop;
  thread into the bubble header + border.
- [x] 2.4 (green) Add eye-glow halo in `renderStone` reusing the
  `mouthGlowMaterial` additive-blend pattern; pulse opacity, color from
  `relay.hue`.
- [x] 2.5 (green) Suppress relay glow while `pokeState === 'angry'` or
  `mode === 'error'` (error/angry wins).
- [x] 2.6 (refactor) Keep relay runtime field (`relayHue`) consistent with the
  existing `pokeState` transient-effect style; dispose new material.

## 3. App wiring

Write scope:

- `src/App.svelte` (pass-through only)

Tasks:

- [x] 3.1 (green) Compute `resolveRelayPresence(...)` from the current bubble
  presentation + connection/primary-agent context and pass its result to
  `<VertexGenie relay={...} />`. No business logic in the component.

## 4. E2E proof

Write scope:

- new spec under `e2e/`

Tasks:

- [x] 4.1 (green) MCP mode: a non-primary agent bubble shows relay attribution +
  relay marker (happy path).
- [x] 4.2 (green) Negative: primary-agent bubble (MCP) shows no relay treatment.
  (API-mode negative is already implicit — `usesMcpConnection` gates it, and the
  default genie fixtures with no configured primary never relay.)
- [x] 4.3 (green) Both pass; full `genie.spec.ts` (13 tests) green.

## 5. Checkpoint

- [x] 5.1 `npm run test:unit` — relay tests 6/6 green; full suite 275/276
  (the one failure, `eckyIrGuide` docs-title drift, is pre-existing and
  unrelated to this change).
- [x] 5.2 Targeted Playwright `genie.spec.ts` green (13/13).
- [x] 5.3 No Rust changed in this change — `cargo check` unaffected, not run.
