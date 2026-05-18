# Delta for direct-occt-runtime

## ADDED Requirements

### Requirement: Standalone OCCT SDK probing

The system SHALL support a standalone OCCT SDK/runtime layout that is not inside
the build123d/OCP Python runtime.

#### Scenario: Explicit OCCT root is configured

- GIVEN `ECKY_OCCT_ROOT` points to a valid OCCT runtime layout
- WHEN runtime capabilities are collected
- THEN direct OCCT is probed from `ECKY_OCCT_ROOT`
- AND no Python site-packages OCP path is required.

#### Scenario: Bundled OCCT runtime exists

- GIVEN app resources contain `runtime/occt`
- WHEN runtime capabilities are collected without `ECKY_OCCT_ROOT`
- THEN direct OCCT is probed from `runtime/occt`.

#### Scenario: Standalone OCCT runtime is invalid

- GIVEN the standalone OCCT runtime manifest or library set is incomplete
- WHEN runtime capabilities are collected
- THEN direct OCCT is reported unavailable
- AND the blocker names the missing manifest field, header, or library.

### Requirement: OCCT runtime manifest

The system SHALL validate a platform-specific OCCT runtime manifest before
declaring native OCCT available.

#### Scenario: Manifest validates

- GIVEN `runtime/occt/manifest.json` declares platform, arch, OCCT version, ABI
  tag, include directory, library directory, required headers, and required
  libraries
- WHEN the SDK probe runs
- THEN every declared required file is checked
- AND the runtime is accepted only if all required files exist.

#### Scenario: Manifest rejects wrong platform

- GIVEN the runtime manifest platform or architecture does not match the current
  host
- WHEN the SDK probe runs
- THEN direct OCCT is reported unavailable
- AND the blocker names the platform or architecture mismatch.

### Requirement: Dependency removal proof gate

The system SHALL prohibit dependency removal tasks until native OCCT proof gates
pass.

#### Scenario: Worker attempts dependency removal early

- GIVEN native OCCT has not passed all proof gates
- WHEN an implementation task attempts to remove build123d, OCP, FreeCAD, or
  Python CAD runners
- THEN the task is out of scope
- AND implementation must stop or be redirected.

### Requirement: Native runtime error surfacing

The system SHALL surface raw native runtime failure details through backend and
UI status paths.

#### Scenario: Native compile fails

- GIVEN the native runner or generated shim fails to compile
- WHEN render reports failure
- THEN the error includes compiler stderr or blocker details
- AND the UI does not replace it with a generic message.

#### Scenario: Native execution fails

- GIVEN native execution exits unsuccessfully
- WHEN render reports failure
- THEN the error includes native stdout/stderr and exit status.
