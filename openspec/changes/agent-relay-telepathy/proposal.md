# Proposal: Agent Relay Telepathy

## Intent

When a non-primary agent speaks in MCP mode, surface its message through the
single Ecky mascot with a "telepathy relay" treatment instead of spawning a
second mascot. The relay must read as *another agent transmitting through Ecky*:
Ecky's eyes glow in the sending agent's signature color and the bubble is
attributed to that agent.

This is the cheap, single-mascot alternative to the multi-mascot
"guests fly in from a portal" idea, which is explicitly abandoned for being
over-built. One viewport, one mascot, a visual effect that conveys *who* is
talking without new layout, queues, or stacked bubbles.

## Scope

- Add a transient `relay` overlay to `VertexGenie` layered on top of the current
  `mode` (the same pattern as the existing `pokeState` runtime flag), not a new
  `GenieMode`.
- Drive the relay glow color from the sending agent's deterministic
  `colorHue` via `buildAgentGenieTraits(senderIdentity)`.
- Attribute the relayed bubble to the sending agent (label + border tint).
- Detect "relay" in the bubble resolution path: an agent/thread-sourced bubble
  whose agent is not the primary agent.
- Gate the entire effect to MCP connections only. API mode bubbles are unchanged.

## Out of Scope

- Multiple mascot instances / guest mascots / portals.
- A bubble queue or stacking model ("queue of queues").
- Changing the single-winner bubble resolver into a per-agent resolver.
- Any new viewport ownership rules — Ecky stays the only mascot, the active
  thread still owns the viewport.
- Sleeping-mode policy changes for MCP (tracked separately if pursued).
- Audio/TTS changes.

## Approach

- `VertexGenie` gains one prop, `relay: { hue: number; label: string } | null`.
  When set, eye grooves get an additive glow halo (reusing the existing
  `mouthGlowMaterial` additive-blend pattern) pulsing in `relay.hue`, and the
  bubble speaker line shows the sending agent's label with a hue-tinted border.
- A pure helper resolves relay state from the already-computed bubble
  presentation plus connection/primary-agent context, returning the sender hue
  and label or `null`. This keeps the single-winner resolver untouched.
- The relay overlay shares the mascot's transient-effect timing model
  (`relayStartedAt`-style runtime field) and clears when the bubble it
  describes is replaced or dismissed.

## Proof Gates

- In MCP mode, a bubble sourced from a non-primary agent renders with the relay
  glow and that agent's attribution label.
- The relay glow color is deterministic for a given agent identity (same
  identity → same hue) and visually distinct from the angry/error treatment
  (cool pulse in agent hue, no red shake).
- In MCP mode, a bubble from the primary agent renders with no relay treatment.
- In API mode, no bubble ever renders the relay treatment.
- The relay overlay clears when its bubble is dismissed or replaced.
- `npm run test:unit`, targeted Playwright, and `cd src-tauri && cargo check`
  pass before completion.
