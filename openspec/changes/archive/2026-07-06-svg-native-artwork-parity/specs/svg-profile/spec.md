# Delta for svg-profile

## ADDED Requirements

### Requirement: Tolerant wire soup extraction

The system SHALL be able to extract an SVG's filled-path wires without rejecting
self-intersecting, multi-subpath, or multi-outer-loop artwork, for backends that
resolve regions downstream.

#### Scenario: Self-intersecting filled path extracts as a wire

- GIVEN an SVG file containing a filled path whose contour self-intersects
- WHEN tolerant wire extraction runs
- THEN the wire is returned without a self-intersection rejection
- AND no Rust-side region resolution is applied to it.

#### Scenario: Multi-subpath evenodd path extracts all subpaths

- GIVEN an SVG `<path>` with multiple `M` subpaths and `fill-rule:evenodd`
- WHEN tolerant wire extraction runs
- THEN every closed subpath is returned as a wire
- AND no single-outer-loop restriction is applied.

#### Scenario: Compound of disjoint filled contours extracts all outers

- GIVEN an SVG file with several disjoint filled contours
- WHEN tolerant wire extraction runs
- THEN each outer contour is returned as a wire
- AND the result is not rejected for having multiple outer loops.

### Requirement: Native region resolution via OCCT planar faces

For Direct OCCT rendering, the system SHALL resolve the SVG wire soup into faces
by delegating to OCCT's planar face builder, matching the behavior of the
build123d/ocpsvg Python path.

#### Scenario: Self-intersecting wire resolves to valid faces

- GIVEN an SVG wire soup containing a self-intersecting filled wire
- WHEN native region resolution runs
- THEN OCCT's planar face builder splits the wire into valid regions
- AND the output is one or more non-self-intersecting faces.

#### Scenario: Even-odd nesting resolves holes by parity of depth

- GIVEN a wire soup with nested contours
- WHEN native region resolution runs
- THEN holes are assigned to outers by containment and parity of nesting depth
- AND the resulting faces carry inner wires for the holes.

#### Scenario: Unfaceable wire is dropped, not fatal

- GIVEN a wire that OCCT cannot convert to a planar face
- WHEN native region resolution runs
- THEN that wire is dropped with a warning
- AND the render succeeds with the faces that OCCT did produce.

## MODIFIED Requirements

### Requirement: Invalid SVG rejection

The system SHALL reject SVG input whose profile interpretation is unambiguously
invalid, while tolerating artwork whose ambiguity is resolved downstream by OCCT.

#### Scenario: Raster-only SVG rejects

- GIVEN an SVG file that contains only raster image data
- WHEN SVG ingestion runs
- THEN ingestion fails
- AND the error states that raster image data is unsupported for vector ingestion.

#### Scenario: Entirely open stroke-only path rejects (no fill)

- GIVEN an SVG path with no fill and no stroke-width ribbon option
- WHEN SVG ingestion runs
- THEN ingestion fails
- AND the error identifies the open, unfilled contour.

#### Scenario: Self-intersecting or multi-outer filled path does NOT reject for native render

- GIVEN an SVG filled path that self-intersects or produces multiple outer loops
- WHEN native Direct OCCT ingestion runs
- THEN ingestion does not reject the path for self-intersection or multi-outer
  reasons
- AND region resolution is delegated to OCCT as specified in
  "Native region resolution via OCCT planar faces".
