## ADDED Requirements

### Requirement: Round primitives accept geometry dimensions only

`cylinder`, `cone`, `circle`, and `ring` SHALL accept exactly two, three,
one, and two positional dimensions respectively. They SHALL NOT accept a
segment count, silently ignore one, or offer a compatibility warning path.
Existing supported alignment keywords SHALL remain valid.

#### Scenario: Captive nut pocket cannot masquerade as a faceted cylinder
- GIVEN `(cylinder 4.85mm 3.6mm 6)` as a nut-pocket cutter
- WHEN source is compiled
- THEN the compiler rejects the extra argument before rendering
- AND the error identifies `cylinder`.

#### Scenario: Hexagonal pocket is explicit geometry
- GIVEN `(extrude (regular-polygon 6 4.85mm) 3.6mm)`
- WHEN source is compiled and rendered by native OCCT
- THEN the result is a six-sided prism.

#### Scenario: Agent receives current primitive syntax
- GIVEN an MCP/API language guide or human reference
- WHEN it lists round primitives
- THEN signatures and worked examples contain dimensions only
- AND the guide explains polygonal geometry through `regular-polygon`.
