## ADDED Requirements

### Requirement: Display-tier commit preserves per-part identity, bounds, and format capability

Display-tier parameter Apply SHALL commit a version using coarse tessellation while preserving per-part STL assets, per-part bounding boxes, and accurate geometry representation provenance (`analyticBrep`, `hybrid`, or `meshNative`).

#### Scenario: Multi-part Apply preserves Solo isolation and per-part bounds

- **GIVEN** a saved multi-part Ecky model (`left`, `right`, `stand`)
- **WHEN** the user applies a parameter edit through the display-tier render path
- **THEN** the committed version stores `productionPending: true` with no STEP export artifact yet
- **AND** each part receives its own coarse STL viewer asset and distinct per-part bounding box
- **AND** Solo isolation and viewport picking target each part individually before the first export

#### Scenario: Mesh-only display commit reports STEP unavailable

- **GIVEN** an Ecky model whose evaluated parts are all mesh-native
- **WHEN** the model is committed on the display tier
- **THEN** the bundle provenance records `meshNative`
- **AND** export options report STEP as disabled instead of promising on-demand BRep STEP

### Requirement: Rust owns on-demand production materialization for exact consumers

Export and FEM analysis consumers SHALL resolve exact production artifacts through one Rust materialization service against the saved revision's source, parameters, component lock, and external mesh digests.

#### Scenario: First STEP export materializes exact production artifacts

- **GIVEN** a saved revision committed with `productionPending: true`
- **WHEN** the user exports STEP or production STL
- **THEN** Rust materializes the production bundle with fine STL, exact STEP, and topology artifacts
- **AND** persists the materialized bundle on that revision so subsequent exports reuse it

#### Scenario: FEM analysis transparently materializes a display-tier revision

- **GIVEN** a saved revision committed with `productionPending: true` and an authored FEM analysis
- **WHEN** the user or MCP runs FEM validation, mesh preview, study, convergence, or topology optimization
- **THEN** Rust materializes the exact production bundle before reading `model.step` and `analysis-boundary.msgpack`
- **AND** FEM runs without failing due to a missing STEP file

#### Scenario: External STL input change blocks silent revision divergence

- **GIVEN** a display-tier revision whose saved bundle recorded external STL source mesh digests
- **WHEN** the external STL file on disk is modified before deferred production materialization
- **THEN** Rust detects the digest mismatch and rejects materialization with a conflict error instead of silently exporting different geometry under the old revision
