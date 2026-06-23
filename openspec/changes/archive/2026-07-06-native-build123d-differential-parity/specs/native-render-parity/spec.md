# Delta for native-render-parity

## ADDED Requirements

### Requirement: Differential geometry parity against build123d

The system SHALL verify Direct OCCT (native) renders by comparing their
preview STL against a build123d render of the identical source and parameters,
within fixed tolerances (volume ±2 %, surface area ±5 %, per-axis bbox
±0.5 mm, identical connected-component count, zero non-manifold edges in the
native mesh).

#### Scenario: Matching render passes the differential check

- GIVEN a macro that renders successfully through build123d
- WHEN the native backend renders the same source and parameters
- THEN the native STL metrics are within tolerance of the reference
- AND the check passes.

#### Scenario: A lost solid fails the differential check

- GIVEN a macro whose build123d reference contains N parts
- WHEN the native render silently drops or inverts a solid (e.g. a lid
  swallowed by a boolean)
- THEN volume/bbox/component metrics diverge beyond tolerance
- AND the differential check fails naming the divergent metric.

#### Scenario: Empty image parameters are part of the corpus

- GIVEN a macro with `image` params left at their empty-string default
- WHEN the differential corpus runs
- THEN the empty-param variant is compared against its own build123d
  reference, not only the params-set variant.

### Requirement: Native render time envelope relative to build123d

The system SHALL bound native render wall time by
`max(10 s, 3 × build123d wall time)` for every differential corpus fixture.

#### Scenario: Native render within the envelope passes

- GIVEN a corpus fixture whose build123d reference renders in T seconds
- WHEN the native render completes within max(10 s, 3 × T)
- THEN the timing check passes.

#### Scenario: Native render exceeding the envelope fails

- GIVEN a corpus fixture whose native render takes longer than the envelope
- WHEN the differential corpus runs
- THEN the timing check fails reporting both durations.
