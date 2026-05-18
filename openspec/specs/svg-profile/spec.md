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

The system SHALL reject SVG input that cannot produce deterministic profile
loops.

#### Scenario: SVG contains raster image only

- GIVEN an SVG file that only embeds raster image data
- WHEN SVG profile ingestion runs
- THEN ingestion fails
- AND the error states that raster SVG content is not supported by profile
  ingestion.

#### Scenario: SVG path is open

- GIVEN an SVG file containing an open path without explicit stroke conversion
- WHEN SVG profile ingestion runs
- THEN ingestion fails
- AND the error names the open contour condition.
