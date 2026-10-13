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

#### Scenario: A completed boolean produces invalid topology

- GIVEN OCCT completes a boolean but a final analytic part fails BRep validation
- WHEN the native runner prepares exports
- THEN it fails before writing STEP, STL, or topology artifacts
- AND the failure identifies the invalid part and suggests inspecting overlaps
- AND no staged cache transaction is committed as a successful render.

### Requirement: No dependency removal without proof

The system SHALL keep existing working render paths until native OCCT satisfies
the proof gates.

#### Scenario: Native path is incomplete

- GIVEN native OCCT does not yet pass all proof gates
- WHEN implementation work proceeds
- THEN no bundled CAD runtime, external CAD command, or Python CAD runner is
  removed from product behavior.

### Requirement: Polyhedral BRep import

The system SHALL accept externally generated triangle meshes and wrap them as
OCCT polyhedral BRep solids so they can participate in exact boolean operations
alongside NURBS-based solids.

#### Scenario: Import clean mesh as poly BRep

- GIVEN a valid STL triangle mesh with closed topology
- WHEN the OCCT executor processes `solidify(import-stl(path))`
- THEN a polyhedral BRep solid is created and stored in a shape slot
- AND the solid can be referenced by subsequent boolean commands

#### Scenario: Import empty mesh

- GIVEN a mesh file containing zero triangles
- WHEN the OCCT executor processes `solidify(import-stl(path))`
- THEN the command fails with a descriptive error
- AND no partial shape is stored

### Requirement: Hybrid boolean operations

The system SHALL execute boolean operations (cut, fuse, common) over solids of
mixed representation — exact NURBS BRep and polyhedral BRep — in a single
OCCT operation.

#### Scenario: Cut exact solid with poly shell

- GIVEN an exact BRep solid (from extrude) and a polyhedral BRep shell (from
  displaced mesh)
- WHEN a `cut` boolean is executed
- THEN the result is a valid OCCT solid
- AND the result preserves exact faces where no intersection occurred

#### Scenario: Fuse exact solids with poly displacement

- GIVEN multiple exact BRep solids and a polyhedral displacement shell
- WHEN a `fuse` boolean is executed
- THEN the result is a single solid containing both exact and poly faces

### Requirement: Direct OCCT surface operation authority

The system SHALL execute supported BRep surface operations in Direct OCCT
whenever the operand is analytic BRep or solidified poly BRep. The system MUST
NOT route analytic `chamfer` or `fillet` through the Rust mesh evaluator as a
silent fallback.

#### Scenario: Analytic chamfer

- GIVEN an analytic BRep shape and a supported `chamfer` operation
- WHEN the Direct OCCT runtime executes the plan
- THEN `BRepFilletAPI_MakeChamfer` is used
- AND the generated model STL is only a tessellation of the analytic result
- AND the STEP export remains analytic BRep

#### Scenario: Solidified mesh-origin chamfer

- GIVEN a validated mesh-origin island has been converted through
  `solidify(import-stl(...))`
- AND the selected edge set passes mesh-origin surface-op admission
- WHEN a supported `chamfer` operation consumes that island
- THEN Direct OCCT applies the chamfer to the solidified poly BRep
- AND the STEP export is marked as faceted poly BRep

#### Scenario: Dense poly BRep chamfer rejected

- GIVEN a solidified mesh-origin poly BRep with a broad chamfer selector
- AND the selector resolves above the configured selected-edge limit
- WHEN Direct OCCT plan validation runs
- THEN the plan is rejected before `BRepFilletAPI_MakeChamfer`
- AND the error includes selected-edge count, limit, and selector
- AND no alternate mesh evaluator route is attempted

### Requirement: Tessellation at partition boundary

The system SHALL tessellate OCCT exact solids into triangle meshes when
crossing from exact BRep operations to mesh-only operations, at a controllable
tessellation density.

#### Scenario: Tessellate extrude result

- GIVEN an exact BRep solid produced by the pre-boundary sub-tree
- WHEN the partition boundary is crossed
- THEN the solid is tessellated into a triangle mesh
- AND the mesh is passed to the mesh-only operation chain

### Requirement: STEP export with poly faces

The system SHALL export STEP files from hybrid solids where exact faces remain
NURBS-represented and polyhedral faces are exported as faceted BRep faces.

#### Scenario: Hybrid part STEP export

- GIVEN a hybrid solid with both exact and poly faces
- WHEN STEP export runs
- THEN the STEP file contains exact NURBS faces for non-displaced geometry
- AND the STEP file contains faceted faces for displaced geometry

#### Scenario: Poly-only part STEP suppression

- GIVEN a part whose geometry is entirely polyhedral (no exact faces)
- WHEN manifest generation runs
- THEN the part is marked as STL-only or poly-tagged in the manifest

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

### Requirement: Native STEP shape import

Direct OCCT SHALL import host-resolved STEP component bytes through
`STEPControl_Reader` and validate transfer status, roots, shape kind, and BRep
validity before publishing a shape slot.

#### Scenario: Valid STEP solid enters plan

- **WHEN** `import-step` receives a readable valid solid
- **THEN** Direct OCCT publishes a shape slot
- **AND** native placement, booleans, topology, STL, and STEP export can consume
  it

#### Scenario: Multiple solid roots remain compound

- **WHEN** transfer yields multiple solids
- **THEN** Direct OCCT preserves a solid-containing compound
- **AND** does not fuse roots implicitly

#### Scenario: Invalid transfer publishes nothing

- **WHEN** read fails, zero roots transfer, output is null, BRep is invalid, or
  payload is shell-only
- **THEN** native execution fails with exact import/validation stage cause
- **AND** publishes no partial slot or export

### Requirement: Native STEP path has no compatibility fallback

The package component STEP path MUST NOT invoke FreeCAD, convert STEP to STL,
or call `solidify`.

#### Scenario: STEP uses reader directly

- **WHEN** a STEP-backed live component renders
- **THEN** generated execution uses `STEPControl_Reader`
- **AND** no FreeCAD process, `StlAPI_Reader`, or `solidify` runs

#### Scenario: STL bridge remains separate

- **WHEN** an STL mesh enters downstream BRep operations
- **THEN** its path remains `solidify(import-stl(path))`
- **AND** STEP import does not share that step

### Requirement: STEP representation truth

Direct runtime SHALL propagate locked package representation evidence and
merge all contributor representations conservatively.

#### Scenario: Analytic placement remains analytic

- **WHEN** trusted analytic STEP is only placed with analytic authored geometry
- **THEN** bundle, manifest, and STEP export report `analyticBrep`

#### Scenario: Faceted input is never relabeled analytic

- **WHEN** STEP payload declares `facetedPolyBrep`
- **THEN** output reports `facetedPolyBrep` or `hybrid`
- **AND** never reports `analyticBrep`

#### Scenario: Mixed composition is hybrid

- **WHEN** analytic authored geometry combines with faceted/mixed STEP
- **THEN** bundle, manifest, and STEP export report `hybrid`

### Requirement: Imported STEP topology remains native

Imported STEP faces and edges SHALL flow through the existing Direct OCCT
topology reporter with component-origin evidence.

#### Scenario: Package ports resolve without FreeCAD

- **WHEN** package ports target locked imported face/edge evidence
- **THEN** Direct OCCT topology contains resolvable targets
- **AND** port validation uses no FreeCAD-generated topology

### Requirement: Selective immutable BRep cache

The system SHALL cache successful immutable Direct OCCT part roots and admitted
expensive command results by resolved execution identity beneath the complete
bundle cache.

#### Scenario: Complete part hit

- **GIVEN** a validated cached analytic BRep has the current part identity and
  runtime metadata
- **WHEN** a new render requires that part
- **THEN** the runner loads the cached root
- **AND** executes zero kernel commands for the part.

#### Scenario: Expensive command hit

- **GIVEN** a cache-admitted command result has current identity
- **WHEN** a missed part root depends on that command
- **THEN** the runner loads the command result
- **AND** does not execute its dependency closure unless another miss requires
  those dependencies.

### Requirement: Localized parameter invalidation

The system SHALL invalidate only parts and command closures whose resolved
execution identities change after a parameter edit.

#### Scenario: One parameter affects one part

- **GIVEN** a three-part model whose middle part alone references parameter P
- **AND** all three parts rendered and entered the selective cache
- **WHEN** P changes
- **THEN** first and third parts are cache hits with zero executed commands
- **AND** only the middle part dirty closure executes
- **AND** the final bundle contains the updated middle part and unchanged clean
  parts.

#### Scenario: Structural parameter changes graph

- **WHEN** a parameter changes a normalized branch or repeat expansion
- **THEN** fingerprints change for the affected resolved graph
- **AND** obsolete cached nodes are not reused by positional coincidence.

### Requirement: Atomic success-only cache publication

The system SHALL publish selective cache entries only after complete successful
execution, representation validation, metadata creation, and artifact digest
creation.

#### Scenario: Kernel failure

- **WHEN** a cache-admitted command or part fails during execution or validation
- **THEN** no cache entry becomes visible
- **AND** the raw kernel failure is returned.

#### Scenario: Corrupt cache entry

- **WHEN** binary BRep bytes, metadata, runtime identity, or stored digest is
  missing or inconsistent
- **THEN** the entry is rejected as a cache miss
- **AND** normal recomputation produces a fresh atomic entry
- **AND** corrupt geometry never reaches the bundle.

### Requirement: Bounded and versioned selective cache

The system SHALL bound selective cache residency by bytes and SHALL include
cache schema, runner ABI, OCCT runtime, tolerances, imports, and tessellation
policy in admission identity.

#### Scenario: Byte budget exceeded

- **WHEN** successful entries exceed the configured byte budget
- **THEN** least-recently-used entries are evicted until within budget
- **AND** active entries remain pinned until their render completes.

#### Scenario: Backend version changes

- **WHEN** cache schema, runner ABI, or OCCT runtime identity changes
- **THEN** prior selective entries become misses
- **AND** authored source and committed model history remain untouched.

### Requirement: Clean-part tessellation reuse

The system SHALL avoid remeshing a clean cached part solely because another part
changed.

#### Scenario: Multipart localized rerender

- **GIVEN** unchanged parts have validated cached triangulation or per-part
  preview meshes
- **WHEN** another part is rebuilt
- **THEN** clean preview data is reused
- **AND** only changed geometry incurs tessellation work
- **AND** the final preview still represents every part.

### Requirement: Explicit hybrid part representation

The runner SHALL execute a planner-declared mesh-domain Boolean boundary and
cache its canonical indexed result without presenting it as analytic BRep.

#### Scenario: Decorated bracelet lid

- **GIVEN** a planner-declared `decorated-dome` mesh-domain group
- **WHEN** imported indexed relief and analytic dome are united
- **THEN** the Boolean executes in the mesh domain
- **AND** the mesh-domain partial and final lid are eligible for immutable cache
- **AND** representation participates in every cache identity.

#### Scenario: Mixed STEP export

- **GIVEN** one mesh-domain part and one or more analytic BRep parts
- **WHEN** STEP export is requested
- **THEN** the mesh-domain part is emitted as an AP242 tessellated surface set
- **AND** analytic parts retain their native BRep
- **AND** the artifact reports both representations truthfully
- **AND** no mesh-domain part is presented as analytic or faceted BRep.

### Requirement: Localized rerender performance evidence

The system SHALL prove localized parameter reuse with kernel counters and
release timing.

#### Scenario: Three-part localized benchmark

- **WHEN** only the middle-part parameter changes after a cold successful render
- **THEN** clean parts report cache hits and zero command executions
- **AND** localized rerender median is no more than 50 percent of cold full
  render median
- **AND** topology, bounds, volume, STEP, and STL contracts remain valid.
