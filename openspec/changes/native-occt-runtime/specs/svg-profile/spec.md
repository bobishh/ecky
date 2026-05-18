# Delta for svg-profile

## ADDED Requirements

### Requirement: Rust-native SVG profile parsing

The system SHALL parse SVG profile geometry in Rust instead of requiring Python
CAD runtime SVG import.

#### Scenario: SVG with one outer loop parses

- GIVEN an SVG file with one visible closed vector contour
- WHEN SVG profile ingestion runs
- THEN ingestion returns one outer loop
- AND the loop coordinates are in model-space units.

#### Scenario: SVG with hole parses

- GIVEN an SVG file with one outer loop and one contained hole loop
- WHEN SVG profile ingestion runs
- THEN ingestion returns one outer loop
- AND ingestion returns one hole loop
- AND loop orientation is normalized deterministically.

#### Scenario: SVG viewBox fits target size

- GIVEN an SVG file with a `viewBox`
- AND the caller supplies target width and height
- WHEN SVG profile ingestion runs with `fit contain`
- THEN the output profile fits inside the target dimensions
- AND aspect ratio is preserved.

### Requirement: SVG profile to exact CAD extrusion

The system SHALL make SVG profile results usable by direct OCCT extrusion.

#### Scenario: SVG profile extrudes through direct OCCT

- GIVEN an SVG profile result
- AND a positive extrusion height
- WHEN direct OCCT rendering runs
- THEN the profile is converted into a face
- AND the face is extruded along Z
- AND the output includes a STEP artifact.

### Requirement: SVG ambiguity rejection

The system SHALL reject SVG input whose profile interpretation is ambiguous.

#### Scenario: Raster-only SVG rejects

- GIVEN an SVG file that contains only raster image data
- WHEN SVG profile ingestion runs
- THEN ingestion fails
- AND the error states that raster image data is unsupported for vector profile
  ingestion.

#### Scenario: Open path rejects

- GIVEN an SVG file with an open contour and no stroke conversion option
- WHEN SVG profile ingestion runs
- THEN ingestion fails
- AND the error identifies the open contour.

#### Scenario: Multi-outer-loop SVG rejects in first slice

- GIVEN an SVG file containing multiple disjoint filled outer contours
- WHEN SVG profile ingestion runs before compound profile support exists
- THEN ingestion fails
- AND the error states that multiple outer loops are unsupported in this slice.

#### Scenario: Self-intersection rejects

- GIVEN an SVG file containing a self-intersecting filled path
- WHEN SVG profile ingestion runs
- THEN ingestion fails
- AND the error identifies invalid loop geometry.

### Requirement: SVG is not raster height extrusion

The system SHALL not map SVG pixels or raster brightness to extrusion height in
SVG profile ingestion.

#### Scenario: Caller requests profile extrusion

- GIVEN vector SVG input
- WHEN the caller extrudes the SVG profile by height `h`
- THEN all profile faces extrude by the same Z height `h`
- AND no per-pixel height sampling occurs.
