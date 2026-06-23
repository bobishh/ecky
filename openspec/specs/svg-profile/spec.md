# svg-profile Specification

## Purpose

Convert vector SVG artwork into deterministic 2D profile loops that can be used
by exact CAD operations.
## Requirements
### Requirement: SVG profile ingestion

The system SHALL treat SVG artwork as vector profile geometry, not raster height
pixels.

#### Scenario: SVG profile parses

- GIVEN an SVG file containing visible closed vector paths
- WHEN SVG profile ingestion runs
- THEN the result contains one outer profile loop
- AND the result may contain hole loops.

#### Scenario: SVG profile extrudes

- GIVEN an SVG profile result and an extrusion height
- WHEN exact CAD rendering runs
- THEN the profile is converted into a planar face
- AND the face is extruded along Z by the requested height.

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

### Requirement: Exact curve segments for native SVG profiles

The system SHALL represent curved SVG contours for Direct OCCT rendering as
exact curve segments (lines and cubic Béziers), not as flattened polylines,
matching build123d/ocpsvg `import_svg` behavior.

#### Scenario: Curved contour emits a bezier-path wire

- GIVEN an SVG filled path containing at least one quadratic or cubic segment
- WHEN the native plan is emitted
- THEN the contour becomes a `bezier-path` node with 3n+1 control points
- AND no polygon node densified by curve sampling is emitted for it.

#### Scenario: Straight-edge contour keeps its polygon plan

- GIVEN an SVG filled path consisting only of line segments
- WHEN the native plan is emitted
- THEN the contour emits the same `polygon` node as before this change.

#### Scenario: Validity checks still run on flattened samples

- GIVEN a curved SVG contour
- WHEN clean-path validity checks (self-intersection, hole containment) run
- THEN they operate on flattened sample points
- AND the emitted geometry still carries the exact curve segments.

### Requirement: Exact curve segments for native text profiles

The system SHALL represent glyph outlines for Direct OCCT rendering as exact
cubic Bézier segments, with TTF quadratic segments elevated to cubics
losslessly, matching OCCT `Font_BRepFont` fidelity.

#### Scenario: Curved glyph emits bezier-path wires

- GIVEN a text op whose glyphs contain curved outline segments
- WHEN the native plan is emitted
- THEN each curved glyph contour becomes a `bezier-path` node
- AND glyph counters (holes) keep resolving to the same outer contours.

#### Scenario: Artwork render cost scales with segment count

- GIVEN dense curved artwork (many curved subpaths)
- WHEN the native wire-soup resolver classifies containment
- THEN classification visits segment endpoints rather than flattened sample
  points
- AND the render completes within the same order of time as the build123d
  backend for the same input.

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

