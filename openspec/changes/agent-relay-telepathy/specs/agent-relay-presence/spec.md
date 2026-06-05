# Delta for agent-relay-presence

## ADDED Requirements

### Requirement: Non-primary agent messages relay through Ecky

The system SHALL surface a message from a non-primary agent through the single
Ecky mascot using a relay treatment, rather than spawning an additional mascot.

#### Scenario: Non-primary agent speaks in MCP mode

- GIVEN an MCP connection with a primary agent
- AND a different (non-primary) agent produces a thread-sourced bubble
- WHEN the bubble becomes the visible advisor bubble
- THEN Ecky renders the relay glow treatment
- AND the bubble is attributed to the sending agent's label.

#### Scenario: Primary agent speaks in MCP mode

- GIVEN an MCP connection with a primary agent
- AND the primary agent produces the visible bubble
- WHEN the bubble is shown
- THEN no relay treatment is applied
- AND the bubble keeps the default Ecky attribution.

#### Scenario: Relay never applies outside MCP

- GIVEN a non-MCP (API) connection
- WHEN any agent bubble is shown
- THEN no relay treatment is applied.

### Requirement: Relay color identifies the sender deterministically

The system SHALL color the relay glow using the sending agent's deterministic
signature hue derived from its identity.

#### Scenario: Same identity yields same hue

- GIVEN two relayed bubbles from the same agent identity
- WHEN each renders the relay treatment
- THEN both use the same glow hue.

#### Scenario: Relay is visually distinct from error

- GIVEN Ecky is in the angry or error state
- WHEN a non-primary agent message would otherwise relay
- THEN the error/angry treatment takes precedence
- AND the relay glow is suppressed.

### Requirement: Relay lifetime follows the bubble

The system SHALL keep the relay treatment active only while the bubble it
attributes is visible.

#### Scenario: Relay clears on dismiss

- GIVEN a relayed bubble is showing the relay treatment
- WHEN the user dismisses the bubble
- THEN the relay treatment is removed.

#### Scenario: Relay re-targets on bubble change

- GIVEN a relayed bubble from agent A is showing
- WHEN a new bubble from a different source replaces it
- THEN the relay treatment is re-derived for the new bubble
- AND no stale glow from agent A remains.
