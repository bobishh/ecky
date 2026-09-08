# Tasks: Hybrid Render Performance and Job Control

## 1. Benchmark and observability

- [ ] Add a stable dense imported-mesh fixture with provenance. Existing CC0
  ladybug asset may be used only as load data; it is not a Poly BRep acceptance
  requirement.
- [x] Record real-app baseline: cold 165.19 s, first disk-cache reuse after
  restart 38.94 s, hot in-memory reuse 30.99 s; 29,614 output triangles,
  3 components, 0 non-manifold edges.
- [ ] Add cold stage benchmark for import, validate, solidify, Boolean, cleanup,
  mesh, verify, and export.
- [x] Add fixed ordered stage-report contract with explicit skipped status,
  execution counts, elapsed milliseconds, and total elapsed time.
- [ ] Record vertices, triangles, OCCT faces, components, manifold edges,
  bounding box, signed volume, and elapsed time.
- [ ] Make current multi-minute path fail an acceptance threshold before more
  optimization.

## 2. OCCT Boolean planner

- [x] Replace sequential union/difference folds with one n-ary argument/tool
  builder while preserving head-minus-tail difference semantics.
- [ ] Add `BOPAlgo_CellsBuilder` proof before changing n-way intersection.
- [x] Enable `SetRunParallel(true)` and `SetUseOBB(true)` in both OCCT paths.
- [x] Add differential topology and volume tests for both execution paths.
- [x] Execute `solidify(import-stl(...))` in the precompiled runner.
- [ ] Benchmark `SetNonDestructive(true)` memory/time before selecting default.
- [ ] Keep glue and fuzzy tolerance disabled without named policies and
  regression fixtures.

## 3. Faceted BRep cleanup

- [ ] Benchmark pre- and post-Boolean same-domain unification.
- [ ] Enable cleanup only behind a proven face-count/cost policy.
- [ ] Assert manifoldness, components, bounds, volume, and deviation.

## 4. Artifact cache and singleflight

- [x] Reuse complete artifact bundles by content and runtime identity.
- [x] Add stored per-artifact digests; reject same-size mutation.
- [ ] Add selective caches for validated mesh, solidified faceted BRep, and
  completed hybrid island.
- [x] Coalesce concurrent identical renders.
- [x] Cache successes only and deliver raw failure to all waiters.
- [x] Include imported bytes and runner/runtime identity in flight and artifact
  keys.
- [x] Snapshot render configuration once for admission and execution.
- [x] Add byte-budgeted eviction and backend-version invalidation tests.

## 5. Progress and cancellation

- [x] Move recursive Direct OCCT normalization to named large-stack worker and
  reject pathological depth.
- [ ] Emit typed stage progress without kernel output in app logs.
- [ ] Bridge OCCT progress and user break where supported.
- [ ] Kill uncooperative kernel child on cancellation.
- [ ] Keep shared work alive until final subscriber cancels.
- [ ] Add happy, failure, pending, and cancellation acceptance tests.

## 6. Representation-aware mesh Boolean route

- [x] Introduce canonical versioned indexed-mesh handoff and sidecar validation.
- [x] Add deterministic IrMesh conversion, digest, bounds, degeneracy,
  topology, component provenance, and Boolean admission.
- [x] Validate named seam welding, orientation, manifoldness, and components;
  require Manifold status after construction and Boolean.
- [x] Record AST-owned Boolean boundaries with authored operand order.
- [x] Route admitted islands through ordered Manifold Boolean folds and retain
  mesh-native output.
- [x] Add indexed decoders for standalone STL/3MF assets.
- [x] Add multipart mesh-native bundle export. End-to-end through the public
  command boundary, all in `src-tauri/`:
  (1) Representation contract in `src/ecky_ir/mesh_asset.rs`:
  `MultipartMeshNativeBundle` emits one canonical `IndexedMeshAsset` per
  authored component with deterministic per-component identity (authored index
  + content digest), preserved `MeshAssetSource` provenance, an order-sensitive
  deterministic bundle digest, `GeometryRepresentation::MeshNative`, and a
  no-fabricated-STEP proof hook. Verified by
  `multipart_mesh_native_bundle_exports_each_component_with_identity_and_provenance_without_step`,
  `multipart_mesh_native_bundle_rejects_empty_component_set`,
  `multipart_bundle_identity_is_label_independent_but_provenance_sensitive`,
  and `multipart_component_id_is_unique_per_authored_index`.
  (2) Bundle-driven encoders in `src/commands/render.rs`:
  `export_mesh_native_bundle_as_3mf_impl` / `export_mesh_native_bundle_as_stl_zip_impl`
  reuse the existing multipart encoders (`write_multipart_3mf_package`,
  `write_binary_stl_triangles`) directly from the canonical indexed mesh per
  component — preserving authored indexing/topology verbatim instead of
  re-indexing lossy STL soup — and never produce STEP. Verified by
  `mesh_native_bundle_export_preserves_component_identity_and_provenance_without_step`
  and `mesh_native_bundle_export_as_stl_zip_preserves_each_component_without_step`.
  (3) Wired public path with NO `ExportPartInput` contract change:
  `try_mesh_native_bundle_from_adjacent_sidecars` auto-discovers a canonical
  indexed-mesh sidecar (`{key}.indexed-mesh.json`) adjacent to each part STL
  (`{key}.stl`), reusing the existing sidecar schema/content-digest validation
  (`IndexedMeshAsset::read_cache`). `export_multipart_3mf_impl` and
  `export_multipart_stl_zip_impl` route through the bundle helpers when every
  part has a valid sidecar; fall back to the current STL-only path when any
  part lacks one (legacy behavior preserved exactly, no per-part
  representation mixing); and fail raw/actionable when any sidecar is
  present-but-invalid (never silently downgrade). Verified at the public
  boundary by
  `export_multipart_3mf_uses_adjacent_indexed_mesh_sidecar_preserving_canonical_topology`
  (canonical triangle index space preserved byte-exact, not STL-reindexed; no
  STEP; deterministic replay),
  `export_multipart_stl_zip_uses_adjacent_indexed_mesh_sidecar_per_component`
  (authored offset preserved, not localized; no STEP; deterministic),
  `export_multipart_rejects_malformed_adjacent_sidecar_without_silent_downgrade`
  (broken sidecar fails raw, no partial artifact), and
  `export_multipart_legacy_part_without_sidecar_keeps_stl_path_behavior`.
  Existing legacy STL/3MF export tests stay green (no sidecars → STL path).
  Producer note (not blocking the export contract): the export side auto-fires
  once a render/runner flow writes `{key}.indexed-mesh.json` next to each
  multipart part STL; the hybrid mesh phase already writes such sidecars for
  its mesh islands.
- [x] Preserve exact BRep/analytic STEP in OCCT and avoid fabricated STEP.
- [x] Reject hidden kernel fallback.
- [x] Centralize Direct OCCT plan admission and report exact part, output slot,
  operation, and ABI constraint before runner execution.

## 7. Optional decoration simplification

- [ ] Add meshoptimizer only if remaining benchmark cost proves need.
- [ ] Use absolute millimetre error, protected fit vertices/borders, and
  requested/achieved error provenance.
- [ ] Reject pruning and silent simplification for physical/fit geometry.

## 8. Preview aggregate payload

- [ ] Pass stable snapshot references or lightweight projections through actor
  boundaries instead of cloning the full artifact graph.
- [ ] Expose dense mesh picking lazily with truncation metadata; do not publish
  every anonymous triangle-derived OCCT target.
- [x] Reject truncated `target_macro_get` windows submitted as full replacement
  without acknowledgement.
- [ ] Add timing assertions for disk-cache and process-hot preview responses.

## Proof gates

- [ ] Cold dense-mesh render meets recorded threshold.
- [x] Warm identical render performs no kernel execution.
- [x] Concurrent identical renders execute one kernel job.
- [ ] Cancellation leaves no orphan process or partial cache entry.
- [x] Final output has zero non-manifold edges and expected components.
- [ ] Bounds, signed volume, and deviation remain within tolerance.
- [x] `cd src-tauri && cargo check` passes.
- [x] MCP inspect → validate → preview → verify proof passes.
- [x] `openspec validate hybrid-render-performance-job-control --strict`.
