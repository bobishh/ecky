import assert from 'node:assert/strict';
import test from 'node:test';

import {
  basename,
  buildSketchDraftRequest,
  clientPointToSvgPoint,
  closeStroke,
  finishStroke,
  normalizePanePoint,
  summarizeSketchDraftMode,
  sourceLineCount,
} from './sketchWorkspaceState';

test('finishStroke closes profile when endpoint returns near start', () => {
  const stroke = finishStroke({
    primitiveId: 'primitive-front-1',
    view: 'front',
    points: [
      [20, 20],
      [60, 20],
      [60, 60],
      [20, 60],
      [21, 21],
    ],
    closed: false,
  });

  assert.equal(stroke.closed, true);
  assert.deepEqual(stroke.points.at(-1), [20, 20]);
});

test('buildSketchDraftRequest rejects open profile before backend call', () => {
  const request = buildSketchDraftRequest([
    {
      primitiveId: 'primitive-front-1',
      view: 'front',
      points: [
        [20, 20],
        [60, 60],
      ],
      closed: false,
    },
  ]);

  assert.deepEqual(request, { error: 'Close profile before preview.' });
});

test('closeStroke closes an open profile explicitly', () => {
  const stroke = closeStroke({
    primitiveId: 'primitive-front-1',
    view: 'front',
    points: [
      [20, 20],
      [60, 20],
      [60, 60],
      [20, 60],
    ],
    closed: false,
  });

  assert.equal(stroke.closed, true);
  assert.deepEqual(stroke.points.at(-1), [20, 20]);
});

test('buildSketchDraftRequest uses first closed primitive and front view', () => {
  const request = buildSketchDraftRequest([
    {
      primitiveId: 'primitive-front-1',
      view: 'front',
      points: [
        [20, 20],
        [60, 20],
        [60, 60],
        [20, 20],
      ],
      closed: true,
    },
  ]);

  assert.ok(!('error' in request));
  assert.equal(request.sketch.view, 'front');
  assert.equal(request.sketch.primitives?.[0]?.primitiveId, 'primitive-front-1');
  assert.equal(request.sketch.primitives?.[0]?.closed, true);
});

test('buildSketchDraftRequest includes dimension constraints for locked width and height', () => {
  const request = buildSketchDraftRequest([
    {
      primitiveId: 'primitive-front-1',
      view: 'front',
      points: [
        [10, 10],
        [60, 10],
        [60, 40],
        [10, 40],
        [10, 10],
      ],
      closed: true,
      dimensionLocks: { width: true, height: true },
    },
  ]);

  assert.ok(!('error' in request));
  assert.deepEqual(request.sketch.constraints, [
    { constraintId: 'primitive-front-1-closed', kind: 'closed', targetIds: ['primitive-front-1'] },
    { constraintId: 'primitive-front-1-width-dimension', kind: 'dimension', targetIds: ['primitive-front-1'], value: 50 },
    { constraintId: 'primitive-front-1-height-dimension', kind: 'dimension', targetIds: ['primitive-front-1'], value: 30 },
  ]);
});

test('buildSketchDraftRequest can preview first closed profile while later sketch guides remain open', () => {
  const request = buildSketchDraftRequest([
    {
      primitiveId: 'primitive-front-1',
      view: 'front',
      points: [
        [20, 20],
        [60, 20],
        [60, 60],
        [20, 20],
      ],
      closed: true,
    },
    {
      primitiveId: 'primitive-side-2',
      view: 'side',
      points: [
        [10, 10],
        [40, 40],
      ],
      closed: false,
    },
  ]);

  assert.ok(!('error' in request));
  assert.equal(request.sketch.primitives?.[0]?.primitiveId, 'primitive-front-1');
});

test('buildSketchDraftRequest keeps multiple closed front primitives for hole-aware replay', () => {
  const request = buildSketchDraftRequest([
    {
      primitiveId: 'primitive-front-outer',
      view: 'front',
      points: [
        [0, 0],
        [80, 0],
        [80, 50],
        [0, 50],
        [0, 0],
      ],
      closed: true,
      topology: {
        loopId: 'front-outer',
        edgeIds: ['outer-a', 'outer-b', 'outer-c', 'outer-d'],
        loopRole: 'outer',
        sourceClass: 'derived',
      },
    },
    {
      primitiveId: 'primitive-front-hole',
      view: 'front',
      points: [
        [25, 18],
        [45, 18],
        [45, 34],
        [25, 34],
        [25, 18],
      ],
      closed: true,
      topology: {
        loopId: 'front-hole',
        edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
        loopRole: 'hole',
        sourceClass: 'derived',
      },
    },
  ]);

  assert.ok(!('error' in request));
  assert.deepEqual(
    request.sketch.primitives?.map((primitive) => primitive.primitiveId),
    ['primitive-front-outer', 'primitive-front-hole'],
  );
  assert.deepEqual(
    request.sketch.primitives?.map((primitive) => primitive.topology?.loopId),
    ['front-outer', 'front-hole'],
  );
  assert.deepEqual(
    request.sketch.primitives?.map((primitive) => primitive.topology?.loopRole),
    ['outer', 'hole'],
  );
});

test('buildSketchDraftRequest preserves replayed sketchId for imported sketch profiles', () => {
  const request = buildSketchDraftRequest([
    {
      primitiveId: 'primitive-front-outer',
      sketchId: 'sketch-alpha',
      view: 'front',
      points: [
        [0, 0],
        [80, 0],
        [80, 50],
        [0, 50],
        [0, 0],
      ],
      closed: true,
    },
    {
      primitiveId: 'primitive-front-hole',
      sketchId: 'sketch-alpha',
      view: 'front',
      points: [
        [25, 18],
        [45, 18],
        [45, 34],
        [25, 34],
        [25, 18],
      ],
      closed: true,
    },
  ]);

  assert.ok(!('error' in request));
  assert.equal(request.sketch.sketchId, 'sketch-alpha');
  assert.deepEqual(
    request.sketch.primitives?.map((primitive) => primitive.primitiveId),
    ['primitive-front-outer', 'primitive-front-hole'],
  );
});

test('buildSketchDraftRequest uses top view depth when front and top profiles match width', () => {
  const request = buildSketchDraftRequest([
    {
      primitiveId: 'primitive-front-1',
      view: 'front',
      points: [
        [10, 20],
        [60, 20],
        [60, 50],
        [10, 50],
        [10, 20],
      ],
      closed: true,
    },
    {
      primitiveId: 'primitive-top-2',
      view: 'top',
      points: [
        [10, 10],
        [60, 10],
        [60, 32],
        [10, 32],
        [10, 10],
      ],
      closed: true,
    },
  ]);

  assert.ok(!('error' in request));
  assert.equal(request.amount, 22);
  assert.equal(request.sketch.view, 'front');
  assert.equal(request.sketch.primitives?.[0]?.primitiveId, 'primitive-front-1');
});

test('buildSketchDraftRequest rejects mismatched front and top widths before backend', () => {
  const request = buildSketchDraftRequest([
    {
      primitiveId: 'primitive-front-1',
      view: 'front',
      points: [
        [10, 20],
        [60, 20],
        [60, 50],
        [10, 50],
        [10, 20],
      ],
      closed: true,
    },
    {
      primitiveId: 'primitive-top-2',
      view: 'top',
      points: [
        [10, 10],
        [50, 10],
        [50, 32],
        [10, 32],
        [10, 10],
      ],
      closed: true,
    },
  ]);

  assert.ok('error' in request);
  assert.equal(request.error, 'Top view width 40mm must match Front view width 50mm.');
  assert.deepEqual(request.repairAction, {
    kind: 'scaleViewAxis',
    primitiveId: 'primitive-top-2',
    view: 'top',
    axis: 'x',
    sourceView: 'front',
    current: 40,
    target: 50,
    message: 'Top view width 40mm must match Front view width 50mm.',
  });
});

test('buildSketchDraftRequest rejects matching front and top widths with shifted x range before backend', () => {
  const request = buildSketchDraftRequest([
    {
      primitiveId: 'primitive-front-1',
      view: 'front',
      points: [
        [10, 20],
        [60, 20],
        [60, 50],
        [10, 50],
        [10, 20],
      ],
      closed: true,
    },
    {
      primitiveId: 'primitive-top-shifted',
      view: 'top',
      points: [
        [30, 10],
        [80, 10],
        [80, 32],
        [30, 32],
        [30, 10],
      ],
      closed: true,
    },
  ]);

  assert.ok('error' in request);
  assert.equal(request.error, 'Top view x range 30..80mm must match Front view x range 10..60mm.');
  assert.deepEqual(request.repairAction, {
    kind: 'translateViewAxisRange',
    primitiveId: 'primitive-top-shifted',
    view: 'top',
    axis: 'x',
    sourceView: 'front',
    currentMin: 30,
    currentMax: 80,
    targetMin: 10,
    targetMax: 60,
    message: 'Top view x range 30..80mm must match Front view x range 10..60mm.',
  });
});

test('summarizeSketchDraftMode labels single and multi view honestly', () => {
  assert.deepEqual(
    summarizeSketchDraftMode([
      {
        primitiveId: 'primitive-front-1',
        view: 'front',
        points: [
          [10, 20],
          [60, 20],
          [60, 50],
          [10, 50],
          [10, 20],
        ],
        closed: true,
      },
    ]),
    { mode: 'single-view', label: 'SINGLE-VIEW EXTRUDE', detail: 'DEPTH 12MM default.' },
  );

  assert.deepEqual(
    summarizeSketchDraftMode([
      {
        primitiveId: 'primitive-front-1',
        view: 'front',
        points: [
          [10, 20],
          [60, 20],
          [60, 50],
          [10, 50],
          [10, 20],
        ],
        closed: true,
      },
      {
        primitiveId: 'primitive-top-2',
        view: 'top',
        points: [
          [10, 10],
          [60, 10],
          [60, 32],
          [10, 32],
          [10, 10],
        ],
        closed: true,
      },
    ]),
    { mode: 'multi-view', label: 'MULTI-VIEW CONSTRAINED', detail: 'DEPTH 22MM from TOP view.' },
  );
});

test('normalizePanePoint maps client coordinates into pane percent space', () => {
  assert.deepEqual(normalizePanePoint(150, 250, { left: 100, top: 200, width: 200, height: 100 }), [25, 50]);
});

test('clientPointToSvgPoint maps client coordinates through SVG screen transform', () => {
  const svg = {
    createSVGPoint() {
      return {
        x: 0,
        y: 0,
        matrixTransform(matrix: unknown) {
          const transform = matrix as { scaleX: number; scaleY: number; translateX: number; translateY: number };
          return {
            x: (this.x - transform.translateX) / transform.scaleX,
            y: (this.y - transform.translateY) / transform.scaleY,
            matrixTransform: this.matrixTransform,
          };
        },
      };
    },
    getScreenCTM() {
      return {
        inverse() {
          return { scaleX: 2, scaleY: 4, translateX: 100, translateY: 200 };
        },
      };
    },
  };

  assert.deepEqual(clientPointToSvgPoint(150, 260, svg), [25, 15]);
});

test('clientPointToSvgPoint clamps SVG coordinates to sketch viewbox', () => {
  const svg = {
    createSVGPoint() {
      return {
        x: 0,
        y: 0,
        matrixTransform() {
          return { x: 130, y: -20, matrixTransform: this.matrixTransform };
        },
      };
    },
    getScreenCTM() {
      return {
        inverse() {
          return {};
        },
      };
    },
  };

  assert.deepEqual(clientPointToSvgPoint(150, 260, svg), [100, 0]);
});

test('sourceLineCount and basename summarize generated preview without full path wall', () => {
  assert.equal(sourceLineCount('one\ntwo\nthree'), 3);
  assert.equal(basename('/tmp/ecky/sketch-preview.stl'), 'sketch-preview.stl');
});
