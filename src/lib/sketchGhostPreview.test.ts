import assert from 'node:assert/strict';
import test from 'node:test';

import { summarizeSketchGhostPreview } from './sketchGhostPreview';

test('summarizeSketchGhostPreview returns null without stroke points', () => {
  assert.equal(
    summarizeSketchGhostPreview({
      activeStroke: null,
      strokes: [],
    }),
    null,
  );
});

test('summarizeSketchGhostPreview uses active open stroke before latest stroke', () => {
  const preview = summarizeSketchGhostPreview({
    activeStroke: {
      view: 'front',
      points: [
        [1, 2],
        [3.5, 4],
      ],
      closed: false,
    },
    strokes: [
      {
        view: 'top',
        points: [
          [10, 10],
          [20, 20],
          [10, 10],
        ],
        closed: true,
      },
    ],
    extrudeDepth: 8,
  });

  assert.deepEqual(preview, {
    status: 'open',
    label: 'OPEN PROFILE',
    view: 'front',
    points: [
      [1, 2],
      [3.5, 4],
    ],
    closed: false,
    path: 'M 1 2 L 3.5 4',
    extrudeDepth: 8,
  });
});

test('summarizeSketchGhostPreview falls back to latest closed stroke', () => {
  const preview = summarizeSketchGhostPreview({
    strokes: [
      {
        view: 'front',
        points: [
          [0, 0],
          [5, 0],
        ],
        closed: false,
      },
      {
        view: 'side',
        points: [
          [0, 0],
          [10, 0],
          [10, 10],
          [0, 0],
        ],
        closed: true,
      },
    ],
  });

  assert.deepEqual(preview, {
    status: 'closed',
    label: 'CLOSED PROFILE',
    view: 'side',
    points: [
      [0, 0],
      [10, 0],
      [10, 10],
      [0, 0],
    ],
    closed: true,
    path: 'M 0 0 L 10 0 L 10 10 Z',
    extrudeDepth: 12,
  });
});

test('summarizeSketchGhostPreview reports queued closed preview', () => {
  const preview = summarizeSketchGhostPreview({
    autoQueued: true,
    strokes: [
      {
        view: 'top',
        points: [
          [1, 1],
          [2, 1],
          [1, 1],
        ],
        closed: true,
      },
    ],
  });

  assert.equal(preview?.status, 'queued');
  assert.equal(preview?.label, 'AUTO-PREVIEW QUEUED');
  assert.equal(preview?.closed, true);
});

test('summarizeSketchGhostPreview reports generating before queued', () => {
  const preview = summarizeSketchGhostPreview({
    generating: true,
    autoQueued: true,
    strokes: [
      {
        view: 'top',
        points: [
          [1, 1],
          [2, 1],
          [1, 1],
        ],
        closed: true,
      },
    ],
  });

  assert.equal(preview?.status, 'generating');
  assert.equal(preview?.label, 'GENERATING PREVIEW');
});

test('summarizeSketchGhostPreview normalizes path number formatting', () => {
  const preview = summarizeSketchGhostPreview({
    strokes: [
      {
        view: 'custom',
        points: [
          [-0, 1.23456],
          [2.5, 3.00001],
        ],
        closed: false,
      },
    ],
  });

  assert.equal(preview?.path, 'M 0 1.2346 L 2.5 3');
});
