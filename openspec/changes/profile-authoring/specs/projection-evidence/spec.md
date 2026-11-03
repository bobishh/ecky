## ADDED Requirements

### Requirement: Derive exact or approximate profile geometry with explicit provenance

The first-release geometry derivation SHALL identify source artifact path/digest, operation, tolerance, units, and accuracy (`exactBrep` or `approximateMesh`). Exact projection/section results SHALL reference an immutable digest-addressed STEP profile-face asset that retains the supported BRep curves and holes. Sampled loops SHALL be marked `approximateDisplayOnly`; they are for display and selection only. Approximate mesh sections SHALL return editable millimeter-valued point loops and SHALL never claim exact BRep geometry.

#### Scenario: Exact STEP section
- **GIVEN** an eligible STEP artifact containing a planar section with a curved boundary and a hole
- **WHEN** the user extracts an exact section on a supported plane and offset
- **THEN** the response reports `exactBrep` and references an immutable profile asset that preserves the exact boundary and hole

#### Scenario: Exact canonical projection
- **GIVEN** an eligible exact BRep/STEP artifact with a closed visible outer contour and hole on a supported canonical view
- **WHEN** the user derives a profile from the projection
- **THEN** the response reports `exactBrep` and references a profile asset built from exact visible geometry while hidden-line edges remain display evidence only

#### Scenario: Mesh section from a scan
- **GIVEN** a photogrammetry or other STL mesh
- **WHEN** the user extracts a planar section
- **THEN** the result is labeled `approximateMesh`, returns millimeter-valued point loops, and does not claim exact geometry even when the mesh came from a CAD export

### Requirement: Exact asset geometry remains authoritative through feature operations

The system SHALL preserve exact profile asset curves and loop topology during exact offset/clearance and extrusion. Exact operations SHALL use the profile asset reference and SHALL NOT use sampled display points as feature geometry. Converting the display overlay into an editable polygon SHALL require an explicit action and change its accuracy to approximate.

Exact profile creation SHALL verify that the target model's selected geometry backend supports the exact profile import operation before persisting source. Unsupported backends SHALL return an actionable diagnostic without creating a failed version.

#### Scenario: Apply exact profile without polygon conversion
- **GIVEN** exact profile evidence with a digest-addressed STEP face asset
- **WHEN** the user applies numeric clearance and extrusion
- **THEN** the generated source imports the exact profile asset and preserves its curve geometry and hole topology without submitting display points

#### Scenario: Explicit polygon conversion
- **GIVEN** exact profile evidence displayed with sampled loops
- **WHEN** the user explicitly chooses point editing through polygon conversion
- **THEN** Ecky warns that the result becomes approximate, drops the exact-asset feature path for the edit, and submits editable loops only after conversion

### Requirement: Invalid or stale evidence cannot become profile geometry

The system SHALL derive feature profiles only from closed, unambiguous projection/section loops. Projection bounds and hidden-only lines SHALL NOT be converted into profile geometry. Rust SHALL verify source and asset digests before applying an exact profile.

#### Scenario: Open or ambiguous boundary
- **GIVEN** evidence with a gap, branch, unresolved connectivity, self-intersection, zero area, or non-finite geometry
- **WHEN** profile derivation is requested
- **THEN** no profile asset or editable profile is produced and a diagnostic describes the unsupported topology

#### Scenario: Bounds without loops
- **GIVEN** finite artifact bounds but no valid closed loop
- **WHEN** profile derivation is requested
- **THEN** bounds may be shown as measurements but are not used to fabricate a rectangle or other profile

#### Scenario: Source or asset changes before apply
- **GIVEN** the base model version, referenced source artifact, or exact profile asset changed after derivation
- **WHEN** the profile is applied
- **THEN** Rust rejects the stale source/digest and does not append the profile feature
