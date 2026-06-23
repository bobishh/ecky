# Delta for svg-profile

## ADDED Requirements

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
