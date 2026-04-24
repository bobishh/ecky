import assert from 'node:assert/strict';
import test from 'node:test';

import { buildSketchFitValidationSeed } from './sketchFitValidation';
import type { SketchFitValidationInput } from './sketchFitValidation';

function baseInput(overrides: Partial<SketchFitValidationInput> = {}): SketchFitValidationInput {
  return {
    profilePoints: [
      { x: 10, y: 10 },
      { x: 50, y: 10 },
      { x: 50, y: 30 },
      { x: 10, y: 30 },
      { x: 10, y: 10 },
    ],
    view: { width: 100, height: 80 },
    extrudeDepth: 12,
    ...overrides,
  };
}

test('returns source-backed rows with pass status for contained profile and preview artifact evidence', () => {
  const result = buildSketchFitValidationSeed(baseInput({
    artifactEvidence: {
      previewArtifactPath: '/tmp/model-preview.stl',
      source: 'build123d',
    },
  }));

  assert.equal(result.status, 'pass');
  assert.deepEqual(result.rows.map((row) => row.id), [
    'containment',
    'dimensions',
    'previewArtifact',
  ]);
  assert.equal(result.rows[0]?.status, 'pass');
  assert.equal(result.rows[1]?.status, 'pass');
  assert.equal(result.rows[2]?.status, 'pass');
  assert.equal(result.evidence.containment.centroidInsideProfile, true);
  assert.equal(result.evidence.containment.edgeSafeSamplesInsideProfile, true);
  assert.equal(result.evidence.previewArtifact.previewArtifactPath, '/tmp/model-preview.stl');
});

test('fails containment when centroid or edge-safe samples are outside closed profile', () => {
  const result = buildSketchFitValidationSeed(baseInput({
    profilePoints: [
      { x: 0, y: 0 },
      { x: 30, y: 0 },
      { x: 30, y: 10 },
      { x: 10, y: 10 },
      { x: 10, y: 30 },
      { x: 0, y: 30 },
      { x: 0, y: 0 },
    ],
  }));

  assert.equal(result.status, 'fail');
  assert.equal(result.rows.find((row) => row.id === 'containment')?.status, 'fail');
  assert.equal(result.evidence.containment.centroidInsideProfile, false);
});

test('fails dimensions when width height or depth are not positive beyond tolerance', () => {
  const result = buildSketchFitValidationSeed(baseInput({
    profilePoints: [
      { x: 4, y: 4 },
      { x: 4.0002, y: 4 },
      { x: 4.0002, y: 4.0002 },
      { x: 4, y: 4.0002 },
      { x: 4, y: 4 },
    ],
    extrudeDepth: 0,
    tolerance: 0.001,
  }));

  assert.equal(result.status, 'fail');
  assert.equal(result.rows.find((row) => row.id === 'dimensions')?.status, 'fail');
  assert.deepEqual(result.evidence.dimensions.positiveAxes, {
    width: false,
    height: false,
    depth: false,
  });
});

test('marks preview artifact pending when no artifact evidence exists', () => {
  const result = buildSketchFitValidationSeed(baseInput());

  assert.equal(result.status, 'pending');
  assert.equal(result.rows.find((row) => row.id === 'previewArtifact')?.status, 'pending');
});

test('propagates raw backend error into row and evidence without generic copy', () => {
  const backendError = {
    status: 422,
    body: '{"error":"profile self intersects at segment 3"}',
  };
  const result = buildSketchFitValidationSeed(baseInput({ backendError }));

  assert.equal(result.status, 'fail');
  assert.equal(result.rows.find((row) => row.id === 'previewArtifact')?.status, 'fail');
  assert.deepEqual(result.evidence.backendError, backendError);
  assert.match(result.rows.find((row) => row.id === 'previewArtifact')?.message ?? '', /profile self intersects/);
});
