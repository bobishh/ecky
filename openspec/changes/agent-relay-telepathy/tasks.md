# Tasks: Agent Relay Telepathy

BDD dual-loop. Each slice: failing test first (red), minimum code (green),
refactor green. Run `npm run test:unit` after each green; targeted Playwright +
`cd src-tauri && cargo check` before the final checkpoint.

## 1. Relay detection (pure helper)

Write scope:

- new `src/lib/agents/relayPresence.ts`
- new `src/lib/agents/relayPresence.test.ts`

Tasks:

- [ ] 1.1 (red) Unit test: `resolveRelayPresence` returns `null` when connection
  is not MCP, for any agent/source.
- [ ] 1.2 (red) Unit test: returns `null` when the bubble source is not
  agent/thread provenance (`threadAgentActivity` | `threadAgentMascot` |
  `threadError`).
- [ ] 1.3 (red) Unit test: returns `null` when the sending agent IS the primary
  agent (via `promptBelongsToPrimaryAgent`).
- [ ] 1.4 (red) Unit test: returns `{ hue, label }` for an MCP, non-primary,
  agent-sourced bubble, where `hue === buildAgentGenieTraits(identity).colorHue`.
- [ ] 1.5 (red) Unit test: same identity → same hue across calls.
- [ ] 1.6 (green) Implement `resolveRelayPresence`; reuse `usesMcpConnection`,
  `promptBelongsToPrimaryAgent`, `buildAgentGenieTraits`.
- [ ] 1.7 (refactor) De-dupe predicates, keep the function pure.

## 2. VertexGenie relay overlay

Write scope:

- `src/lib/VertexGenie.svelte`
- relay-related rendering unit coverage where feasible

Tasks:

- [ ] 2.1 (red) Component/DOM test: with `relay` prop set, the bubble shows the
  sending agent's attribution label instead of the default `ECKY EINACS:`.
- [ ] 2.2 (red) Component/DOM test: with `relay` set, the bubble carries a
  hue-tinted relay marker (e.g. data attribute / class) for the relay state.
- [ ] 2.3 (green) Add `relay: { hue: number; label: string } | null` prop;
  thread into the bubble header + border.
- [ ] 2.4 (green) Add eye-glow halo in `renderStone` reusing the
  `mouthGlowMaterial` additive-blend pattern; pulse opacity, color from
  `relay.hue`.
- [ ] 2.5 (green) Suppress relay glow while `pokeState === 'angry'` or
  `mode === 'error'` (error/angry wins).
- [ ] 2.6 (refactor) Keep relay runtime field consistent with the existing
  `pokeState`/`pokeStartedAt` transient-effect style; dispose new geometry.

## 3. App wiring

Write scope:

- `src/App.svelte` (pass-through only)

Tasks:

- [ ] 3.1 (green) Compute `resolveRelayPresence(...)` from the current bubble
  presentation + connection/primary-agent context and pass its result to
  `<VertexGenie relay={...} />`. No business logic in the component.

## 4. E2E proof

Write scope:

- new spec under `e2e/`

Tasks:

- [ ] 4.1 (red) MCP mode: a non-primary agent bubble shows relay attribution +
  relay marker (happy path).
- [ ] 4.2 (red) Negative: primary-agent bubble (MCP) and any API-mode bubble
  show no relay treatment.
- [ ] 4.3 (green) Make both pass; clean up.

## 5. Checkpoint

- [ ] 5.1 `npm run test:unit` green.
- [ ] 5.2 Targeted Playwright spec green.
- [ ] 5.3 `cd src-tauri && cargo check` clean (no Rust change expected; confirm).
