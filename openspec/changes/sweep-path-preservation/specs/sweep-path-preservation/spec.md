## ADDED Requirements

### Requirement: Preserve linear path corners

The native sweep MUST include both endpoints and every direction-changing vertex from
an all-linear spine in its section sequence, in wire order.

#### Scenario: Three-point authored corner

- GIVEN a circular profile and path `(0,0,0) -> (20,0,0) -> (20,20,0)`
- WHEN native sweep evaluates the path
- THEN the result MUST be a valid solid whose bounds and volume reflect both
  path segments
- AND the implementation MUST NOT replace the corner with a diagonal shortcut

#### Scenario: Multiple authored corners

- GIVEN a five-point orthogonal polyline with four segments
- WHEN native sweep evaluates the path
- THEN the result MUST remain a valid solid with volume greater than the
  corresponding diagonal-shortcut result

### Requirement: Bound linear sweep work

The native sweep MUST remove only numerically collinear interior samples, then
reject an all-linear spine with more than 1024 remaining section points before
constructing the section loft.

#### Scenario: Oversized section sequence

- GIVEN an all-linear spine with 1025 non-collinear points
- WHEN native sweep evaluates the path
- THEN it MUST fail with an error identifying the maximum of 1024 sections

### Requirement: Preserve round sampled sections

The native sweep MUST retain the circular profile geometry for a sampled Bézier
polyline under the section bound.

#### Scenario: Sampled curved path

- GIVEN a cubic Bézier path represented by the existing 16 linear samples and a
  radius-4.5 circular profile
- WHEN native sweep evaluates the path
- THEN the result MUST be a valid solid with circular section edges of radius 4.5
- AND a plane intersection through the middle of the curved span MUST measure
  approximately 9 by 9
- AND bounds and volume MUST be measured by the native acceptance test

### Requirement: Keep existing helix behavior

The native sweep MUST continue using the dedicated Frenet branch for `frenet=true`
paths; linear section preservation MUST NOT alter its section count or frame logic.

#### Scenario: Frenet sweep

- **GIVEN** a helix sweep with `frenet=true`
- **WHEN** native sweep evaluates the path
- **THEN** the existing Frenet implementation owns its frames and sections
- **AND** the linear-section budget does not replace that branch.
