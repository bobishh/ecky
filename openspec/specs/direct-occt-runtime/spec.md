# direct-occt-runtime Specification

## Purpose

Manage the native OpenCascade runtime used to render Ecky Core IR into BREP
artifacts.

## Requirements

### Requirement: Runtime capability probing

The system SHALL report whether the direct OCCT runtime can compile and execute
the native export shim.

#### Scenario: Runtime probe succeeds

- GIVEN the configured runtime root contains required OCCT headers and libraries
- WHEN runtime capabilities are collected
- THEN direct OCCT is reported available
- AND the capability detail identifies the usable runtime path.

#### Scenario: Runtime probe fails

- GIVEN the configured runtime root is missing required OCCT headers or libraries
- WHEN runtime capabilities are collected
- THEN direct OCCT is reported unavailable
- AND the capability detail includes the raw blocker summary.

### Requirement: Runtime bundle output

The system SHALL create runtime bundles only after successful native OCCT export.

#### Scenario: Export succeeds

- GIVEN a Core IR model supported by direct OCCT
- WHEN native export succeeds
- THEN the runtime bundle contains an STL preview
- AND the runtime bundle contains a STEP artifact
- AND the runtime bundle contains topology evidence.

#### Scenario: Export fails

- GIVEN native export fails during compile, run, STEP write, STL write, or
  topology write
- WHEN runtime bundle creation aborts
- THEN partial bundle output is removed
- AND the raw native failure detail is surfaced.

### Requirement: No dependency removal without proof

The system SHALL keep existing working render paths until native OCCT satisfies
the proof gates.

#### Scenario: Native path is incomplete

- GIVEN native OCCT does not yet pass all proof gates
- WHEN implementation work proceeds
- THEN no bundled CAD runtime, external CAD command, or Python CAD runner is
  removed from product behavior.
