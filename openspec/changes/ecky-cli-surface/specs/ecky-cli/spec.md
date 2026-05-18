# ecky-cli Specification

## ADDED Requirements

### Requirement: Source check command

The system SHALL provide a CLI command that validates `.ecky` source without
rendering.

#### Scenario: Check succeeds

- GIVEN a valid `.ecky` source file
- WHEN the user runs `ecky check model.ecky`
- THEN the command exits `0`
- AND stdout reports a successful compile summary

#### Scenario: Check fails

- GIVEN an invalid `.ecky` source file
- WHEN the user runs `ecky check broken.ecky`
- THEN the command exits non-zero
- AND stderr includes the raw compiler diagnostic

### Requirement: Lower command

The system SHALL provide a CLI command that lowers `.ecky` source into backend
source code without rendering geometry.

#### Scenario: Lower to build123d file

- GIVEN a valid `.ecky` source file
- WHEN the user runs `ecky lower --backend build123d model.ecky --out out.py`
- THEN the command exits `0`
- AND `out.py` is written

#### Scenario: Lower to FreeCAD stdout

- GIVEN a valid `.ecky` source file
- WHEN the user runs `ecky lower --backend freecad model.ecky`
- THEN the command exits `0`
- AND stdout contains the lowered FreeCAD macro source

### Requirement: Render command

The system SHALL provide a CLI command that renders `.ecky` source through a
selected backend and writes requested artifacts.

#### Scenario: Render with explicit params

- GIVEN a valid `.ecky` source file with parameterized geometry
- WHEN the user runs `ecky render --backend build123d model.ecky --param width=42 --stl out.stl`
- THEN the command exits `0`
- AND `out.stl` is written

#### Scenario: Render with params file

- GIVEN a valid `.ecky` source file
- AND a JSON params file
- WHEN the user runs `ecky render --backend freecad model.ecky --params params.json --stl out.stl`
- THEN the command exits `0`
- AND `out.stl` is written

#### Scenario: Backend render fails

- GIVEN source or backend state that fails render
- WHEN the user runs `ecky render ...`
- THEN the command exits non-zero
- AND stderr preserves the raw backend/runtime failure detail

### Requirement: Parameter override parsing

The system SHALL parse repeatable CLI parameter overrides and merge them with
optional JSON param input.

#### Scenario: CLI flag overrides JSON file

- GIVEN a params file that sets `width=20`
- AND a CLI flag `--param width=42`
- WHEN the user runs `ecky render ... --params params.json --param width=42`
- THEN effective render parameters use `width=42`

#### Scenario: Malformed param is rejected

- GIVEN an invalid CLI param token
- WHEN the user runs `ecky render ... --param width`
- THEN the command exits non-zero
- AND stderr explains the malformed `key=value` input
