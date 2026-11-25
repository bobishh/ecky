## 1. Spike: Display-to-Production Seam

- [ ] 1.1 Benchmark Direct OCCT preview deflection vs production STEP/STL across benchmark corpus (`worm-farm`).
- [x] 1.2 Validate that coarse display mesh produces consistent bounding boxes and manifold faces without visually misleading gaps.
- [ ] 1.3 Prove deferred materialization produces byte-deterministic STEP/STL matching immediate execution.

## 2. Backend & Contract Support

- [x] 2.1 Update `ArtifactBundle` to allow deferred `model_step_path` with `production_pending` flag.
- [x] 2.2 Add `materialize_production_bundle` service triggered lazily on export.
- [x] 2.3 Wire `Apply` to request display-tier rendering only.

## 3. UI & Export Verification

- [x] 3.1 Show instantaneous parameter Apply feedback in workbench.
- [x] 3.2 Show progress indicator during Export only when production materialization is triggered.
- [x] 3.3 Add automated outside-in test covering edit -> instant display -> export -> valid STEP.
