import assert from 'node:assert/strict';
import test from 'node:test';

import { buildSketchDimensionSummary } from './sketchDimensionSummary';

test('buildSketchDimensionSummary reports rectangle width height depth and closed constraint', () => {
  const summary = buildSketchDimensionSummary(
    {
      view: 'front',
      points: [
        [10, 20],
        [50, 20],
        [50, 55],
        [10, 55],
        [10, 20],
      ],
      closed: true,
    },
    12,
  );

  assert.deepEqual(summary, {
    view: 'front',
    width: 40,
    height: 35,
    depth: 12,
    pointCount: 4,
    constraints: ['CLOSED PROFILE'],
    evidence: ['front view', 'bounds 40mm x 35mm', 'extrude depth 12mm', '4 profile points'],
  });
});

test('buildSketchDimensionSummary derives irregular profile bounds from extremes', () => {
  const summary = buildSketchDimensionSummary(
    {
      view: 'top',
      points: [
        [15, 60],
        [80, 42],
        [74, 90],
        [21, 84],
        [15, 60],
      ],
      closed: true,
    },
    7.5,
  );

  assert.deepEqual(summary, {
    view: 'top',
    width: 65,
    height: 48,
    depth: 7.5,
    pointCount: 4,
    constraints: ['CLOSED PROFILE'],
    evidence: ['top view', 'bounds 65mm x 48mm', 'extrude depth 7.5mm', '4 profile points'],
  });
});

test('buildSketchDimensionSummary returns null for open profile', () => {
  const summary = buildSketchDimensionSummary(
    {
      view: 'front',
      points: [
        [10, 20],
        [50, 20],
        [50, 55],
      ],
      closed: false,
    },
    12,
  );

  assert.equal(summary, null);
});
