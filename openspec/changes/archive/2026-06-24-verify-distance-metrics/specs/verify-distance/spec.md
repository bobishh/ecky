# Delta for verify-distance

## ADDED Requirements

### Requirement: Authored clearance distance metrics resolve named selectors
The system SHALL support a `clearance` authored metric namespace with a
`min-distance` metric key so authors can compare the minimum distance between
two named selectors in `.ecky` source.

#### Scenario: Part selectors compute AABB distance
- GIVEN a verify clause that uses `(metric gap (clearance min-distance part_a part_b))`
- AND both selectors resolve to manifest parts with finite bounds
- WHEN authored verification evaluates the clause
- THEN the system computes the minimum distance between the two part bounds
- AND compares the numeric result with the authored expectation.

#### Scenario: Target selectors resolve through manifest and correspondence evidence
- GIVEN a verify clause that uses two selectors that resolve to selection targets or correspondence outputs
- AND the runtime bundle exposes target geometry for those selectors
- WHEN authored verification evaluates the clause
- THEN the system resolves selectors deterministically through manifest and correspondence evidence
- AND computes minimum distance from the resolved geometry anchors.

#### Scenario: Missing selector evidence fails authored verification
- GIVEN a verify clause that uses a selector name that cannot be resolved to any supported selector evidence
- WHEN authored verification evaluates the clause
- THEN the system reports an authored verify error
- AND the message identifies the unresolved selector.

#### Scenario: Too-small distance fails authored verification
- GIVEN a verify clause that compares a `clearance min-distance` result with a lower bound
- WHEN the resolved minimum distance is below the expected threshold
- THEN the system reports an authored verify failure
- AND the raw structural verification result remains additive.
