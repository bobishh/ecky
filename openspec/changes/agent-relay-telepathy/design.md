# Design: Agent Relay Telepathy

## Existing foundation (do not rebuild)

- `buildAgentGenieTraits(identity)` (`src/lib/genie/traits.ts:213`) hashes an
  agent identity (FNV-1a → seed) into deterministic `GenieTraits`, including a
  stable `colorHue`. This is the relay color source — no new state needed.
- `VertexGenie` already layers a transient runtime effect (`pokeState` /
  `pokeStartedAt`, `src/lib/VertexGenie.svelte:68`) on top of `mode`. Relay
  follows the same pattern: a runtime field, not a new `GenieMode`.
- The mouth already has an additive-blend glow (`mouthGlowMaterial`,
  `src/lib/VertexGenie.svelte:398`). The eye halo reuses this exact technique.
- `promptBelongsToPrimaryAgent` (`src/lib/agents/state.ts:88`) and
  `usesMcpConnection` (`src/lib/agents/state.ts:28`) already express the two
  gating predicates (is-primary, is-MCP).
- `deriveMascotStateForThreadAgent` (`src/lib/agents/state.ts:184`) and the
  bubble source enum in `src/lib/agents/draftFeedback.ts` already carry the
  thread-agent provenance the resolver keys off.

## Decision 1: overlay, not mode

Relay is a transient overlay flag, not a `GenieMode`. The mode still reflects
*what Ecky is doing* (idle/thinking/rendering/...); relay is orthogonal *who is
speaking through Ecky*. Adding a mode would force a combinatorial palette and
break the existing `resolveModeTraits` switch. The overlay rides on top and only
touches eye glow + bubble chrome.

## Decision 2: relay detection is a pure helper

A new pure function (e.g. `resolveRelayPresence`) takes the already-resolved
`GenieBubblePresentation`, the connection type, the primary-agent context, and
the sending agent's identity/label, and returns
`{ hue, label } | null`. It does **not** alter the single-winner resolver in
`draftFeedback.ts`. Returns `null` unless ALL hold:

- `usesMcpConnection(connectionType)` is true (MCP only).
- The bubble source is agent/thread provenance
  (`threadAgentActivity` | `threadAgentMascot` | `threadError`).
- The sending agent is not the primary agent
  (`!promptBelongsToPrimaryAgent(...)`).

`hue` = `buildAgentGenieTraits(senderIdentity).colorHue`.
`label` = the sending agent's label.

## Decision 3: distinct from angry/error

Angry/error already paints eyes red with a shake (`runtimeAngry` branch,
`src/lib/VertexGenie.svelte:500`). Relay must not collide: relay is a *cool*
additive pulse in the agent's own hue with no positional shake. When both
could apply, angry/error wins (it is about Ecky's own failure state); relay is
suppressed while `pokeState === 'angry'` or `mode === 'error'`.

## Decision 4: relay lifetime tied to the bubble

The relay overlay lives exactly as long as the bubble it attributes. When the
bubble text changes (new winner from the resolver) or is dismissed, relay is
re-derived from the new presentation — so it naturally clears or re-targets. No
independent timer. This avoids a glow that outlives its message.

## Open question (defer, not blocking)

- Whether to also tint the *bubble background* (not just border) in the agent
  hue. Default: border + speaker label only, to stay within the Tactical
  Midnight theme. Revisit if attribution reads weakly in testing.

## Files touched

- `src/lib/VertexGenie.svelte` — `relay` prop, eye glow halo, bubble label/border.
- `src/lib/agents/<relay helper>.ts` (+ `.test.ts`) — `resolveRelayPresence`.
- `src/App.svelte` — wire resolver output → `relay` prop (no logic, pass-through).
- e2e spec under `e2e/` — MCP relay happy path + primary-agent/API negative case.
