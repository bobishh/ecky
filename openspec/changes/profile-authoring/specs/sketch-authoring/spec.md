## ADDED Requirements

### Requirement: Profile edits produce ordinary `.ecky` versions

The first release SHALL submit supported profile features through Rust's normal source/version path. The transient editor SHALL NOT create a separate authoritative sketch document or history.

#### Scenario: Save an exact profile feature
- **GIVEN** an existing flange represented by a supported STEP/BRep artifact with a visible top outline and through-hole
- **WHEN** the user chooses exact top projection, builds its closed profile, enters clearance and thickness, then applies the profile
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

Sketch SHALL fill the application's work area. A compact view selector SHALL offer FRONT, TOP, SIDE, and ALL THREE. The default SHALL be one large FRONT pane; ALL THREE SHALL use the established simultaneous front/top/side arrangement. Layout changes SHALL preserve each view's geometry, edits, camera, and evidence. Other workbench windows SHALL retain their own layout behavior.

For exact CAD projection, one projection action SHALL request all three views for the same selected model version, regardless of the display layout. Each view SHALL retain its own result, pending state, and error; failure in one view SHALL NOT erase successful views. Front maps to XZ, top to XY, and side to YZ.

A projection drawing and a closed profile are distinct results. Project all three views SHALL display native exact visible projected edges without waiting for material-boundary classification, closed-loop assembly, or extrusion preparation. Each edge SHALL be drawn independently; the preview SHALL NOT connect unrelated endpoints or close open edges. The exact projected curves SHALL be retained in a separate content-addressed STEP asset; sampled screen points are preview-only. A drawing asset SHALL NOT be accepted as an extrudable profile asset.

Build closed profile SHALL explicitly request the existing strict native profile derivation for the selected view. Only successful closed-profile derivation SHALL bind that view's profile asset to Apply or enable approximate polygon conversion. Pending or failed profile construction SHALL preserve the projection drawing and other views. Backend errors SHALL show actionable diagnostic text, including process exit status when a native process fails without text output. Unsupported closed-profile extraction SHALL NOT prevent an otherwise valid drawing projection.

Native derivation SHALL retain every valid source solid. Loose STEP faces lacking any surface and belonging to no solid MAY be excluded with an explicit warning. Invalid solid bodies SHALL be reported rather than silently omitted. Projection drawing and profile assets SHALL preserve source digest and view provenance.

Each view SHALL independently own its manual loops, unfinished stroke, undo history, selected vertex, placement offset, and converted-profile state. Selecting another view SHALL NOT transfer a drawing or overwrite another view. Rendering ALL THREE SHALL show all existing drawings concurrently, including inactive views. Each canvas SHALL have exactly one coordinate-aligned grid that follows pan and zoom; no screen-space decorative grid may be layered beneath it.

Camera scale SHALL be defined independently of pane dimensions, in screen pixels per model millimeter, with a per-view model-space center. Resizing the application or changing between single-view and ALL THREE layouts SHALL change the visible model-space extent without changing scale. Fit-to-view is an explicit action, except for initial presentation of newly loaded geometry. A measurement update alone SHALL NOT refit geometry. Grid spacing is metric and its screen spacing follows camera scale; optional coarse/fine grid levels may change with zoom, not with window size.

#### Scenario: Independent drawings and layout switching
- **GIVEN** a rectangle drawn in FRONT and a distinct circle or polyline drawn in TOP
- **WHEN** the user switches among FRONT, TOP, SIDE, and ALL THREE
- **THEN** each drawing remains in its original view with its original geometry and camera
- **AND** ALL THREE shows both drawings simultaneously
- **AND** undoing or editing FRONT does not alter TOP

#### Scenario: Preserve an unfinished stroke and converted profile
- **GIVEN** FRONT contains an unfinished manual stroke and TOP contains an exact profile explicitly converted to editable geometry
- **WHEN** the user edits SIDE and returns to the other views
- **THEN** FRONT can continue its own stroke and TOP retains its edited approximate profile and provenance

#### Scenario: Coordinate grid after pan and zoom
- **GIVEN** a point placed at a known grid coordinate
- **WHEN** the user pans and zooms the pane or changes the display layout
- **THEN** the point remains aligned with that coordinate in a single grid, without a stationary second grid

#### Scenario: Window resize preserves drawing scale
- **GIVEN** two points with a known model-space separation and an established camera scale
- **WHEN** the application is resized or the user changes between single-view and ALL THREE layouts without zooming
- **THEN** their screen-space separation and the grid's screen-space spacing remain unchanged within pixel-rounding tolerance
- **AND** the pane shows a larger or smaller extent of the same drawing
- **WHEN** the user zooms or explicitly fits the view
- **THEN** camera scale and grid spacing may change together

#### Scenario: Project all three views from one model
- **GIVEN** an eligible STEP model is selected in the current project version
- **WHEN** the user projects the model
- **THEN** front, top, and side each receive an exact projection from that same source version and ALL THREE displays them together
- **AND** each pane retains independent loading and error status without moving the sketch layout

#### Scenario: Apply selected top or side projection
- **GIVEN** exact projections are available in the three panes
- **WHEN** the user selects TOP or SIDE, successfully builds its closed profile, and applies clearance and depth
- **THEN** the new feature uses that view's profile asset and operation reference on XY for TOP or YZ for SIDE
- **AND** sampled overlay loops are not submitted as exact geometry

#### Scenario: One projection fails
- **GIVEN** the model can be projected in two views but the third projection is unsupported
- **WHEN** the user requests all views
- **THEN** the failed pane shows its diagnostic while successful drawings remain selectable and available for closed-profile construction

#### Scenario: Drawing succeeds while closed-profile construction fails
- **GIVEN** native projection produces exact visible edges from a STEP assembly, including open curves and internal detail
- **WHEN** the user requests all three views
- **THEN** each successful drawing appears without requiring closed boundaries and Apply remains unavailable for a drawing alone
- **WHEN** Build closed profile fails for the selected drawing
- **THEN** the drawing remains visible, the failure explains the unsupported boundary, and the other views remain available

#### Scenario: Surface-less loose faces in a STEP assembly
- **GIVEN** an assembly contains valid solid bodies and additional loose faces with null surfaces
- **WHEN** the user projects the assembly
- **THEN** all valid solids participate in the native drawing calculation without passing the null-surface faces to HLR
- **AND** the result identifies the excluded loose faces in a warning

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
