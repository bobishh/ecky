# Delta for ecky-convenience-ops

## ADDED Requirements

### Requirement: Convenience primitives are first-class surface ops

The system SHALL accept `torus`, `ellipse`, `regular-polygon`, `slot`,
`trapezoid`, and `wedge` as `.ecky` surface ops that parse, compile to Core IR,
and pass `verify_core_program` arity/type checks like any built-in primitive.

#### Scenario: A convenience primitive compiles

- GIVEN `.ecky` source using a convenience primitive with valid arguments
- WHEN the source is parsed and compiled
- THEN a corresponding Core IR node is produced
- AND `verify_core_program` accepts it.

#### Scenario: A convenience primitive rejects bad arity

- GIVEN `.ecky` source calling a convenience primitive with missing or
  mistyped required arguments
- WHEN the source is compiled
- THEN compilation fails
- AND the diagnostic names the op and its expected signature.

### Requirement: Convenience feature ops operate on existing solids

The system SHALL provide `draft`, `rib`, `groove`, a variable-radius form of
`fillet`, and `thread` as feature ops that consume an input solid (and, where
applicable, a face/edge selector) and produce a modified solid.

#### Scenario: Draft tapers selected faces

- GIVEN a solid and a `draft` clause selecting faces with an angle and neutral plane
- WHEN the model is compiled and rendered on the default backend
- THEN the selected faces are tapered by the given angle
- AND unselected faces are unchanged.

#### Scenario: Variable fillet stays backward compatible

- GIVEN existing `.ecky` source using `fillet` with a single uniform radius
- WHEN the variable-radius form is added
- THEN the existing uniform form continues to compile and render unchanged.

#### Scenario: Thread composes a helical-ridge thread

- GIVEN a `thread` clause with diameter, pitch, length, and handedness
- WHEN the model is rendered on the default backend
- THEN a helical thread of the requested pitch and handedness is produced
- AND male/female variants apply the correct clearance.

#### Scenario: ISO designation decodes to the parametric core

- GIVEN a `thread` clause specifying an ISO metric designation such as `M4`
- WHEN it is compiled
- THEN the designation is decoded via a coarse-pitch table into the same
  parametric pitch/diameter the author could have written by hand
- AND the resulting geometry is identical to that explicit parametric thread.

#### Scenario: Unknown ISO designation is rejected

- GIVEN a `thread` clause with a designation not in the table
- WHEN it is compiled
- THEN compilation fails
- AND the diagnostic names the designation and that it is unsupported.

### Requirement: Every convenience op holds geometry parity across backends

For every convenience op, the system SHALL produce the same solid on the native
OCCT backend and on the build123d backend, within tessellation tolerance.

#### Scenario: Native and build123d agree

- GIVEN a model using a convenience op
- WHEN it is rendered on native OCCT and on build123d
- THEN the two results have equal bounding box and volume within tolerance
- AND neither backend silently drops the op's geometry.

#### Scenario: A native-only op is gated honestly

- GIVEN a convenience op that an interop backend cannot reproduce
- WHEN the model is lowered to that interop backend
- THEN the lowering fails with a clear native-only diagnostic
- AND the op continues to render on the native backend.
