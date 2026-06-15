# Delta for parametric-thread-feature

## ADDED Requirements

### Requirement: External thread is a solid, never a hollow spiral

The system SHALL build an external `thread` as one solid (core fused with the
helical ridge) across all valid parameters, by giving the core a small radial
overlap with the ridge root so the fuse never operates on a coincident face. A
thread SHALL NOT degrade to a hollow ridge that drops the core.

#### Scenario: Coarse, deep thread stays solid

- GIVEN an external thread with coarse pitch and deep engagement
- WHEN it is rendered
- THEN the result is a single connected solid
- AND its volume corresponds to core + ridge (not the ridge alone).

#### Scenario: A hollow result is caught by geometric sanity

- GIVEN a thread that renders as a valid single solid
- WHEN verification runs
- THEN it also checks the solid contains the expected core (volume/core check),
  so a hollow spiral that passes topological validity is still flagged.

### Requirement: Thread profile is intent-derived

The system SHALL let callers specify a thread profile by intent — a flank angle
and a profile mode — and SHALL derive the base/crest widths from the flank and
depth (`base = crest + 2·depth·tan(flank)`), without a numerical solver. Explicit
base/crest widths SHALL still override the derivation.

#### Scenario: Flank angle drives the profile

- GIVEN a thread authored with a flank angle and a depth
- WHEN the flank angle changes
- THEN the derived base width changes accordingly and the geometry rebuilds
- AND no other parameter must be edited by hand.

#### Scenario: Merging turns are diagnosed

- GIVEN a thread whose derived base width exceeds `pitch − clearance`
- WHEN it is authored
- THEN a printability diagnostic reports that the turns will merge
- AND it is a diagnostic, not a hard failure.

### Requirement: A tapped hole is a manifold cutter applied by placement

The system SHALL provide a `tapped-hole` female cutter (a bore at the minor
diameter unioned with a helical relief, the relief inset below the bore so no
coincident face remains) that, when differenced from any body, yields a manifold
internal thread. The cutter SHALL be positioned by the existing placement ops
(`location`/`place`) so a thread can be cut into any wall on a chosen axis.

#### Scenario: Tapped hole in a wall is manifold

- GIVEN a wall body and a `tapped-hole` placed on an axis through it
- WHEN the hole is differenced from the wall
- THEN the result has zero non-manifold edges
- AND it has a through bore at the minor diameter with relief out to the major.

#### Scenario: Mating thread and tapped hole engage

- GIVEN an external thread and a `tapped-hole` of equal nominal size with
  complementary clearance
- WHEN both are built
- THEN their fit dimensions are compatible (the male major fits the female
  cleared minor) so they thread together.

### Requirement: Symmetric and asymmetric profiles in one primitive

The system SHALL support a symmetric profile and an asymmetric buttress profile
under one `thread` primitive selected by a profile mode, where the buttress mode
keeps the overhang flank within the printable angle while allowing a steep load
flank. The buttress profile SHALL be expressible by `helical-ridge` accepting an
asymmetric cross-section.

#### Scenario: Buttress keeps the overhang flank printable

- GIVEN a thread in buttress mode with a 45° return flank and a steep load flank
- WHEN it is rendered for vertical printing
- THEN the downward-facing (overhang) flank is at or under the printable angle
- AND the load flank remains steep.

### Requirement: Printability is expressed as verification

The system SHALL provide reusable `verify` clauses for a printed thread —
single solid, manifold, overhang within budget, and `pitch > base` — so print
readiness is measured, not eyeballed.

#### Scenario: Too-shallow flank fails the overhang clause

- GIVEN a printed thread whose overhang flank is too shallow for the orientation
- WHEN verification runs
- THEN the overhang clause goes red
- AND loosening the flank angle to within budget makes it green.
