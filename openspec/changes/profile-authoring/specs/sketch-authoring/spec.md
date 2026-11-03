## ADDED Requirements

### Requirement: Profile edits produce ordinary `.ecky` versions

The first release SHALL submit supported profile features through Rust's normal source/version path. The transient editor SHALL NOT create a separate authoritative sketch document or history.

#### Scenario: Save an exact profile feature
- **GIVEN** an existing flange represented by a supported STEP/BRep artifact with a visible top outline and through-hole
- **WHEN** the user chooses exact top projection, enters clearance and thickness, then applies the profile
- **THEN** Ecky keeps the flange source, adds a named part through the exact profile asset, and creates a normal immutable version in the same project
- **AND** the generated feature preserves the hole, projection provenance, clearance, and extrusion depth without submitting sampled display points

#### Scenario: Save a manual or approximate profile
- **GIVEN** a manual closed polyline or an approximate mesh section
- **WHEN** the user applies supported clearance and extrusion
- **THEN** Ecky submits editable loops through the guarded normal source/version service with manual or approximate provenance

#### Scenario: Reopen the saved version
- **GIVEN** a profile feature saved in a model version
- **WHEN** the user reopens that version
- **THEN** the saved `.ecky` source and geometry are available through the normal model workflow; automatic rehydration into the transient editor is not required

#### Scenario: Stale target version
- **GIVEN** the target thread head changed after the user opened the profile editor
- **WHEN** the user applies the profile
- **THEN** Rust rejects the stale base version and reports that the user must reload before applying

### Requirement: Compact editor supports exact and approximate profiles

The first release SHALL support manual closed-polyline drawing and vertex editing, exact profile assets from supported BRep/STEP projection and section, and approximate polygon profiles from mesh sections. Exact profile assets SHALL remain authoritative for numeric clearance/offset and extrusion; sampled display loops SHALL NOT be submitted as exact geometry. Point editing of exact sampled loops SHALL require explicit approximate conversion. A full parametric solver, adaptive arbitrary-face attachment, and drawing sheets remain later requirements.

For a derived profile, the source view/plane and section offset SHALL determine the feature plane and location; the editor SHALL keep those fields read-only while that evidence is selected. Manual profiles MAY choose a principal plane and offset. Changing a section plane or offset requires deriving the evidence again.

For exact CAD projection, the editor SHALL preserve the established simultaneous front/top/side sketch panes. One projection action SHALL request all three views for the same selected model version. Each pane SHALL retain its own result, pending state, and error; failure in one view SHALL NOT erase successful views. Selecting a pane SHALL bind its own exact profile reference and canonical plane to Apply. Front maps to XZ, top to XY, and side to YZ. Exact pane overlays remain display-only until explicit approximate polygon conversion.

#### Scenario: Project all three views from one model
- **GIVEN** an eligible STEP model is selected in the current project version
- **WHEN** the user projects the model
- **THEN** front, top, and side panes appear together and each receives an exact projection from that same source version
- **AND** each pane retains independent loading and error status without moving the sketch layout

#### Scenario: Apply selected top or side projection
- **GIVEN** exact projections are available in the three panes
- **WHEN** the user selects TOP or SIDE and applies clearance and depth
- **THEN** the new feature uses that view's profile asset and operation reference on XY for TOP or YZ for SIDE
- **AND** sampled overlay loops are not submitted as exact geometry

#### Scenario: One projection fails
- **GIVEN** the model can be projected in two views but the third projection is unsupported
- **WHEN** the user requests all views
- **THEN** the failed pane shows its diagnostic while successful panes remain selectable and can be applied

#### Scenario: Exact profile controls
- **GIVEN** exact CAD evidence with sampled display contours
- **WHEN** the user adjusts clearance and extrusion depth without converting to a polygon
- **THEN** the exact profile asset remains selected and only its reference plus numeric operations reach the backend

#### Scenario: Convert exact display to polygon
- **GIVEN** an exact CAD profile asset
- **WHEN** the user requests vertex editing
- **THEN** the editor requires explicit approximate conversion and labels the resulting editable profile approximate

#### Scenario: Apply an outer profile with a hole
- **GIVEN** a valid outer loop and its supported inner hole loop
- **WHEN** the user applies a supported profile feature
- **THEN** the generated feature preserves loop roles and parent relationships

#### Scenario: Invalid operation values
- **GIVEN** non-finite offset, negative clearance, or non-positive extrusion depth
- **WHEN** the user requests application
- **THEN** validation rejects the request before creating a source version and returns an actionable diagnostic

#### Scenario: Geometry render fails after submission
- **GIVEN** profile source passed request validation but its geometry fails during rendering
- **WHEN** the normal version workflow records the failure
- **THEN** the failed immutable version retains its error and the previous usable preview remains available

### Requirement: Editor state is scoped and stable

The frontend SHALL discard stale section/projection responses when thread, version, or source scope changes. It SHALL project Rust-owned progress and errors while reserving status space so pending work does not move the editor canvas or primary controls.

#### Scenario: Scope changes during extraction
- **GIVEN** a section or projection request for one source scope is still pending
- **WHEN** the active source scope changes
- **THEN** the old response is not installed into the new edit and controls remain available for the new scope

#### Scenario: Extraction or feature application is pending
- **GIVEN** extraction or version creation is running
- **WHEN** pending status is displayed
- **THEN** status uses reserved space and the canvas and primary controls do not shift
