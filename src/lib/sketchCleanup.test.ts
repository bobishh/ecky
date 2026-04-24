import assert from 'node:assert/strict';
import test from 'node:test';

import { cleanupSketchStrokes } from './sketchCleanup';
import type { SketchStroke } from './sketchWorkspaceState';

test('cleanupSketchStrokes rectangles latest closed stroke and preserves metadata', () => {
  const strokes: SketchStroke[] = [
    {
      primitiveId: 'primitive-front-old',
      view: 'front',
      points: [
        [1, 1],
        [2, 1],
        [2, 2],
        [1, 1],
      ],
      closed: true,
    },
    {
      primitiveId: 'primitive-top-open',
      view: 'top',
      points: [
        [0, 0],
        [10, 10],
      ],
      closed: false,
    },
    {
      primitiveId: 'primitive-front-1',
      view: 'front',
      points: [
        [20, 15],
        [70, 22],
        [63, 45],
        [30, 44],
        [20, 15],
      ],
      closed: true,
      dimensionLocks: { width: true, height: true },
    },
  ];

  const result = cleanupSketchStrokes(strokes);

  assert.deepEqual(result, {
    strokes: [
      strokes[0],
      strokes[1],
      {
        primitiveId: 'primitive-front-1',
        view: 'front',
        points: [
          [20, 15],
          [70, 15],
          [70, 45],
          [20, 45],
          [20, 15],
        ],
        closed: true,
        dimensionLocks: { width: true, height: true },
      },
    ],
    evidence: ['primitive-front-1 cleaned to rectangle width 50mm height 30mm.'],
  });
});

test('cleanupSketchStrokes returns error when no closed profile exists', () => {
  const result = cleanupSketchStrokes([
    {
      primitiveId: 'primitive-front-1',
      view: 'front',
      points: [
        [10, 10],
        [20, 20],
      ],
      closed: false,
    },
  ]);

  assert.deepEqual(result, { error: 'Close profile before cleanup.' });
});

test('cleanupSketchStrokes returns error for zero closed profile bounds', () => {
  const result = cleanupSketchStrokes([
    {
      primitiveId: 'primitive-front-1',
      view: 'front',
      points: [
        [10, 10],
        [10, 20],
        [10, 30],
        [10, 10],
      ],
      closed: true,
    },
  ]);

  assert.deepEqual(result, { error: 'Sketch cleanup needs a non-zero closed profile.' });
});
