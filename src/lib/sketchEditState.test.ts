import assert from 'node:assert/strict';
import test from 'node:test';

import {
  assertLockedDimensionsPreserved,
  canEditStrokePoint,
  deleteClosedStrokePoint,
  editablePointIndices,
  closedStrokeBounds,
  hitTestSketchPoint,
  logicalPointCount,
  moveClosedStrokePoint,
  moveClosedStrokePointCoordinate,
  moveClosedStrokePointWithDimensionLocks,
  normalizeSketchDimension,
  normalizeSketchCoordinate,
  normalizeSketchGridSize,
  resizeClosedStrokeBounds,
  resizeClosedStrokeBoundsSnapped,
  setClosedStrokeBoundsOrigin,
  setClosedStrokeBoundsOriginSnapped,
  snapPointToGrid,
  type SketchStroke,
} from './sketchEditState';

const closedStroke: SketchStroke = {
  primitiveId: 'primitive-front-1',
  view: 'front',
  points: [
    [10, 10],
    [60, 10],
    [60, 60],
    [10, 10],
  ],
  closed: true,
};

const closedQuadStroke: SketchStroke = {
  primitiveId: 'primitive-front-quad',
  view: 'front',
  points: [
    [10, 10],
    [60, 10],
    [60, 60],
    [10, 60],
    [10, 10],
  ],
  closed: true,
};

const openStroke: SketchStroke = {
  primitiveId: 'primitive-front-2',
  view: 'front',
  points: [
    [5, 5],
    [15, 15],
    [25, 25],
  ],
  closed: false,
};

test('editablePointIndices returns logical point indices for open and closed strokes', () => {
  assert.deepEqual(editablePointIndices(openStroke), [0, 1, 2]);
  assert.deepEqual(editablePointIndices(closedStroke), [0, 1, 2]);
});

test('canEditStrokePoint normalizes the closing point on closed strokes', () => {
  assert.equal(canEditStrokePoint(closedStroke, 3), true);
  assert.equal(canEditStrokePoint(closedStroke, -1), false);
  assert.equal(canEditStrokePoint(openStroke, 3), false);
});

test('moveClosedStrokePoint updates the closing point pair and clones arrays', () => {
  const next = moveClosedStrokePoint(closedStroke, 0, [12, 14]);

  assert.notEqual(next, closedStroke);
  assert.notEqual(next.points, closedStroke.points);
  assert.notEqual(next.points[0], closedStroke.points[0]);
  assert.notEqual(next.points[3], closedStroke.points[3]);
  assert.deepEqual(next, {
    primitiveId: 'primitive-front-1',
    view: 'front',
    points: [
      [12, 14],
      [60, 10],
      [60, 60],
      [12, 14],
    ],
    closed: true,
  });
});

test('moveClosedStrokePoint normalizes the final closing point to the first point', () => {
  const next = moveClosedStrokePoint(closedStroke, 3, [18, 21]);

  assert.deepEqual(next.points, [
    [18, 21],
    [60, 10],
    [60, 60],
    [18, 21],
  ]);
});

test('moveClosedStrokePoint rejects invalid point and index with exact errors', () => {
  assert.throws(() => moveClosedStrokePoint(closedStroke, 9, [1, 2]), /Invalid sketch point index\./);
  assert.throws(() => moveClosedStrokePoint(closedStroke, 1, [1, Number.NaN]), /Invalid sketch point\./);
  assert.throws(() => moveClosedStrokePoint(openStroke, 1, [1, 2]), /Closed stroke required\./);
});

test('moveClosedStrokePointWithDimensionLocks without locks behaves like moveClosedStrokePoint', () => {
  const next = moveClosedStrokePointWithDimensionLocks(closedQuadStroke, 1, [80, 24]);
  const expected = moveClosedStrokePoint(closedQuadStroke, 1, [80, 24]);

  assert.deepEqual(next, expected);
  assert.notEqual(next, closedQuadStroke);
  assert.notEqual(next.points, closedQuadStroke.points);
  assert.notEqual(next.points[1], closedQuadStroke.points[1]);
});

test('moveClosedStrokePointWithDimensionLocks translates whole rectangle when both dimensions are locked', () => {
  const lockedStroke: SketchStroke = {
    ...closedQuadStroke,
    dimensionLocks: { width: true, height: true },
  };

  const next = moveClosedStrokePointWithDimensionLocks(lockedStroke, 1, [80, 25]);

  assert.deepEqual(next.points, [
    [30, 25],
    [80, 25],
    [80, 75],
    [30, 75],
    [30, 25],
  ]);
  assert.notEqual(next.points[0], next.points[4]);
  assert.equal(closedStrokeBounds(next).width, closedStrokeBounds(lockedStroke).width);
  assert.equal(closedStrokeBounds(next).height, closedStrokeBounds(lockedStroke).height);
});

test('moveClosedStrokePointWithDimensionLocks translates x and edits y when width is locked', () => {
  const lockedStroke: SketchStroke = {
    ...closedQuadStroke,
    dimensionLocks: { width: true },
  };

  const next = moveClosedStrokePointWithDimensionLocks(lockedStroke, 1, [80, 25]);

  assert.deepEqual(next.points, [
    [30, 10],
    [80, 25],
    [80, 60],
    [30, 60],
    [30, 10],
  ]);
  assert.equal(closedStrokeBounds(next).width, closedStrokeBounds(lockedStroke).width);
});

test('moveClosedStrokePointWithDimensionLocks edits x and translates y when height is locked', () => {
  const lockedStroke: SketchStroke = {
    ...closedQuadStroke,
    dimensionLocks: { height: true },
  };

  const next = moveClosedStrokePointWithDimensionLocks(lockedStroke, 1, [80, 25]);

  assert.deepEqual(next.points, [
    [10, 25],
    [80, 25],
    [60, 75],
    [10, 75],
    [10, 25],
  ]);
  assert.equal(closedStrokeBounds(next).height, closedStrokeBounds(lockedStroke).height);
});

test('moveClosedStrokePointWithDimensionLocks preserves invalid state errors', () => {
  assert.throws(() => moveClosedStrokePointWithDimensionLocks(openStroke, 1, [1, 2]), {
    message: 'Closed stroke required.',
  });
  assert.throws(() => moveClosedStrokePointWithDimensionLocks(closedStroke, 9, [1, 2]), {
    message: 'Invalid sketch point index.',
  });
  assert.throws(() => moveClosedStrokePointWithDimensionLocks(closedStroke, 1, [1, Number.NaN]), {
    message: 'Invalid sketch point.',
  });
});

test('moveClosedStrokePointCoordinate updates one coordinate and clones point arrays', () => {
  const next = moveClosedStrokePointCoordinate(closedStroke, 0, 'x', ' 12.5 ');

  assert.notEqual(next, closedStroke);
  assert.notEqual(next.points, closedStroke.points);
  assert.notEqual(next.points[0], closedStroke.points[0]);
  assert.notEqual(next.points[3], closedStroke.points[3]);
  assert.deepEqual(next, {
    primitiveId: 'primitive-front-1',
    view: 'front',
    points: [
      [12.5, 10],
      [60, 10],
      [60, 60],
      [12.5, 10],
    ],
    closed: true,
  });
});

test('moveClosedStrokePointCoordinate updates y on an inner logical point', () => {
  const next = moveClosedStrokePointCoordinate(closedStroke, 1, 'y', 15.25);

  assert.deepEqual(next.points, [
    [10, 10],
    [60, 15.25],
    [60, 60],
    [10, 10],
  ]);
});

test('moveClosedStrokePointCoordinate normalizes closing point to first logical point', () => {
  const next = moveClosedStrokePointCoordinate(closedStroke, 3, 'y', '21.75');

  assert.deepEqual(next.points, [
    [10, 21.75],
    [60, 10],
    [60, 60],
    [10, 21.75],
  ]);
});

test('moveClosedStrokePointCoordinate rejects invalid axis and preserves existing errors', () => {
  assert.throws(() => moveClosedStrokePointCoordinate(closedStroke, 1, 'z', 1), {
    message: 'Invalid sketch axis.',
  });
  assert.throws(() => moveClosedStrokePointCoordinate(closedStroke, 9, 'x', 1), {
    message: 'Invalid sketch point index.',
  });
  assert.throws(() => moveClosedStrokePointCoordinate(closedStroke, 9, 'z', 1), {
    message: 'Invalid sketch point index.',
  });
  assert.throws(() => moveClosedStrokePointCoordinate(openStroke, 1, 'x', 1), {
    message: 'Closed stroke required.',
  });
  assert.throws(() => moveClosedStrokePointCoordinate(closedStroke, 1, 'x', ''), {
    message: 'Invalid sketch coordinate.',
  });
});

test('snapPointToGrid rounds to the nearest grid increment and clones the point', () => {
  const point = [12, 27] satisfies [number, number];
  const next = snapPointToGrid(point, 10);

  assert.notEqual(next, point);
  assert.deepEqual(next, [10, 30]);
  assert.deepEqual(snapPointToGrid([-14, 14], 5), [-15, 15]);
  assert.deepEqual(snapPointToGrid([7, 10], 3.1415), [6.283, 9.4245]);
});

test('snapPointToGrid rejects invalid grid size and point with exact errors', () => {
  assert.throws(() => snapPointToGrid([1, 2], 0), { message: 'Invalid sketch grid size.' });
  assert.throws(() => snapPointToGrid([1, 2], Number.POSITIVE_INFINITY), {
    message: 'Invalid sketch grid size.',
  });
  assert.throws(() => snapPointToGrid([1, Number.NaN], 10), { message: 'Invalid sketch point.' });
});

test('normalizeSketchGridSize accepts positive finite numeric and string values', () => {
  assert.equal(normalizeSketchGridSize(10), 10);
  assert.equal(normalizeSketchGridSize(2.5), 2.5);
  assert.equal(normalizeSketchGridSize(' 12 '), 12);
  assert.equal(normalizeSketchGridSize(' 2.5 '), 2.5);
  assert.equal(normalizeSketchGridSize('3.1415'), 3.1415);
});

test('normalizeSketchGridSize rejects invalid values with exact error', () => {
  assert.throws(() => normalizeSketchGridSize(0), { message: 'Invalid sketch grid size.' });
  assert.throws(() => normalizeSketchGridSize(-1), { message: 'Invalid sketch grid size.' });
  assert.throws(() => normalizeSketchGridSize(Number.NaN), { message: 'Invalid sketch grid size.' });
  assert.throws(() => normalizeSketchGridSize(Number.POSITIVE_INFINITY), { message: 'Invalid sketch grid size.' });
  assert.throws(() => normalizeSketchGridSize('0'), { message: 'Invalid sketch grid size.' });
  assert.throws(() => normalizeSketchGridSize('-1'), { message: 'Invalid sketch grid size.' });
  assert.throws(() => normalizeSketchGridSize('NaN'), { message: 'Invalid sketch grid size.' });
  assert.throws(() => normalizeSketchGridSize('Infinity'), { message: 'Invalid sketch grid size.' });
  assert.throws(() => normalizeSketchGridSize(''), { message: 'Invalid sketch grid size.' });
  assert.throws(() => normalizeSketchGridSize('   '), { message: 'Invalid sketch grid size.' });
});

test('normalizeSketchCoordinate accepts finite numeric and trimmed string values', () => {
  assert.equal(normalizeSketchCoordinate(10), 10);
  assert.equal(normalizeSketchCoordinate(-2.5), -2.5);
  assert.equal(normalizeSketchCoordinate(' 12 '), 12);
  assert.equal(normalizeSketchCoordinate(' -2.5 '), -2.5);
  assert.equal(normalizeSketchCoordinate('3.1415'), 3.1415);
});

test('normalizeSketchCoordinate rejects invalid values with exact error', () => {
  assert.throws(() => normalizeSketchCoordinate(Number.NaN), { message: 'Invalid sketch coordinate.' });
  assert.throws(() => normalizeSketchCoordinate(Number.POSITIVE_INFINITY), {
    message: 'Invalid sketch coordinate.',
  });
  assert.throws(() => normalizeSketchCoordinate('NaN'), { message: 'Invalid sketch coordinate.' });
  assert.throws(() => normalizeSketchCoordinate('Infinity'), { message: 'Invalid sketch coordinate.' });
  assert.throws(() => normalizeSketchCoordinate(''), { message: 'Invalid sketch coordinate.' });
  assert.throws(() => normalizeSketchCoordinate('   '), { message: 'Invalid sketch coordinate.' });
  assert.throws(() => normalizeSketchCoordinate(null), { message: 'Invalid sketch coordinate.' });
  assert.throws(() => normalizeSketchCoordinate(undefined), { message: 'Invalid sketch coordinate.' });
  assert.throws(() => normalizeSketchCoordinate(true), { message: 'Invalid sketch coordinate.' });
});

test('normalizeSketchDimension accepts positive finite numeric and trimmed string values', () => {
  assert.equal(normalizeSketchDimension(10), 10);
  assert.equal(normalizeSketchDimension(2.5), 2.5);
  assert.equal(normalizeSketchDimension(' 12 '), 12);
  assert.equal(normalizeSketchDimension(' 2.5 '), 2.5);
  assert.equal(normalizeSketchDimension('3.1415'), 3.1415);
});

test('normalizeSketchDimension rejects invalid and non-positive values with exact errors', () => {
  assert.throws(() => normalizeSketchDimension(Number.NaN), { message: 'Invalid sketch dimension.' });
  assert.throws(() => normalizeSketchDimension(Number.POSITIVE_INFINITY), {
    message: 'Invalid sketch dimension.',
  });
  assert.throws(() => normalizeSketchDimension('NaN'), { message: 'Invalid sketch dimension.' });
  assert.throws(() => normalizeSketchDimension('Infinity'), { message: 'Invalid sketch dimension.' });
  assert.throws(() => normalizeSketchDimension(''), { message: 'Invalid sketch dimension.' });
  assert.throws(() => normalizeSketchDimension('   '), { message: 'Invalid sketch dimension.' });
  assert.throws(() => normalizeSketchDimension(null), { message: 'Invalid sketch dimension.' });
  assert.throws(() => normalizeSketchDimension(undefined), { message: 'Invalid sketch dimension.' });
  assert.throws(() => normalizeSketchDimension(true), { message: 'Invalid sketch dimension.' });
  assert.throws(() => normalizeSketchDimension(0), { message: 'Sketch dimension must be positive.' });
  assert.throws(() => normalizeSketchDimension(-1), { message: 'Sketch dimension must be positive.' });
  assert.throws(() => normalizeSketchDimension('0'), { message: 'Sketch dimension must be positive.' });
  assert.throws(() => normalizeSketchDimension('-1'), { message: 'Sketch dimension must be positive.' });
});

test('closedStrokeBounds returns bounds for a closed stroke', () => {
  assert.deepEqual(closedStrokeBounds(closedQuadStroke), {
    minX: 10,
    minY: 10,
    maxX: 60,
    maxY: 60,
    width: 50,
    height: 50,
  });
});

test('closedStrokeBounds rejects open stroke and invalid point with exact errors', () => {
  assert.throws(() => closedStrokeBounds(openStroke), { message: 'Closed stroke required.' });
  assert.throws(
    () =>
      closedStrokeBounds({
        ...closedStroke,
        points: [[10, 10], [Number.NaN, 20], [10, 10]],
      }),
    { message: 'Invalid sketch point.' },
  );
  assert.throws(
    () =>
      closedStrokeBounds({
        ...closedStroke,
        points: [
          [10, 10],
          [60, 10],
          [60, 60],
          [Number.NaN, 10],
        ],
      }),
    { message: 'Invalid sketch point.' },
  );
});

test('resizeClosedStrokeBounds scales from min bounds anchor and clones point arrays', () => {
  const next = resizeClosedStrokeBounds(closedQuadStroke, '100', ' 25.5 ');

  assert.notEqual(next, closedQuadStroke);
  assert.equal(next.primitiveId, closedQuadStroke.primitiveId);
  assert.equal(next.view, closedQuadStroke.view);
  assert.equal(next.closed, true);
  assert.notEqual(next.points, closedQuadStroke.points);
  assert.notEqual(next.points[0], closedQuadStroke.points[0]);
  assert.notEqual(next.points[4], closedQuadStroke.points[4]);
  assert.deepEqual(next.points, [
    [10, 10],
    [110, 10],
    [110, 35.5],
    [10, 35.5],
    [10, 10],
  ]);
});

test('resizeClosedStrokeBounds keeps duplicate close point synchronized', () => {
  const next = resizeClosedStrokeBounds(
    {
      ...closedQuadStroke,
      points: [
        [10, 20],
        [60, 20],
        [60, 70],
        [10, 70],
        [10, 20],
      ],
    },
    25,
    100,
  );

  assert.deepEqual(next.points[0], [10, 20]);
  assert.deepEqual(next.points[4], [10, 20]);
  assert.notEqual(next.points[0], next.points[4]);
});

test('resizeClosedStrokeBoundsSnapped rounds target dimensions to the grid before scaling', () => {
  const next = resizeClosedStrokeBoundsSnapped(closedQuadStroke, '83', '26', '10');

  assert.deepEqual(next.points, [
    [10, 10],
    [90, 10],
    [90, 40],
    [10, 40],
    [10, 10],
  ]);
});

test('resizeClosedStrokeBoundsSnapped preserves exact grid validation error', () => {
  assert.throws(() => resizeClosedStrokeBoundsSnapped(closedQuadStroke, 83, 26, 0), {
    message: 'Invalid sketch grid size.',
  });
});

test('setClosedStrokeBoundsOrigin translates closed stroke to min x/y and preserves dimensions', () => {
  const next = setClosedStrokeBoundsOrigin(closedQuadStroke, '25', ' 35 ');

  assert.notEqual(next, closedQuadStroke);
  assert.notEqual(next.points, closedQuadStroke.points);
  assert.notEqual(next.points[0], closedQuadStroke.points[0]);
  assert.notEqual(next.points[4], closedQuadStroke.points[4]);
  assert.deepEqual(next.points, [
    [25, 35],
    [75, 35],
    [75, 85],
    [25, 85],
    [25, 35],
  ]);
  assert.equal(closedStrokeBounds(next).width, closedStrokeBounds(closedQuadStroke).width);
  assert.equal(closedStrokeBounds(next).height, closedStrokeBounds(closedQuadStroke).height);
});

test('setClosedStrokeBoundsOrigin preserves dimension locks while translating', () => {
  const lockedStroke: SketchStroke = {
    ...closedQuadStroke,
    dimensionLocks: { width: true, height: true },
  };

  const next = setClosedStrokeBoundsOrigin(lockedStroke, 20, 25);

  assert.deepEqual(next, {
    ...lockedStroke,
    points: [
      [20, 25],
      [70, 25],
      [70, 75],
      [20, 75],
      [20, 25],
    ],
  });
});

test('setClosedStrokeBoundsOriginSnapped rounds origin to the grid before translating', () => {
  const next = setClosedStrokeBoundsOriginSnapped(closedQuadStroke, '23', '36', '10');

  assert.deepEqual(next.points, [
    [20, 40],
    [70, 40],
    [70, 90],
    [20, 90],
    [20, 40],
  ]);
});

test('setClosedStrokeBoundsOriginSnapped preserves exact grid validation error', () => {
  assert.throws(() => setClosedStrokeBoundsOriginSnapped(closedQuadStroke, 23, 36, 0), {
    message: 'Invalid sketch grid size.',
  });
});

test('setClosedStrokeBoundsOrigin preserves validation errors with exact messages', () => {
  assert.throws(() => setClosedStrokeBoundsOrigin(openStroke, 10, 10), { message: 'Closed stroke required.' });
  assert.throws(() => setClosedStrokeBoundsOrigin(closedStroke, 'nope', 10), { message: 'Invalid sketch coordinate.' });
  assert.throws(() => setClosedStrokeBoundsOrigin(closedStroke, 10, ''), { message: 'Invalid sketch coordinate.' });
  assert.throws(
    () => setClosedStrokeBoundsOrigin({ ...closedStroke, points: [[10, 10], [Infinity, 20], [10, 10]] }, 10, 10),
    { message: 'Invalid sketch point.' },
  );
});

test('resizeClosedStrokeBounds preserves validation errors with exact messages', () => {
  const flatStroke: SketchStroke = {
    ...closedQuadStroke,
    points: [
      [10, 10],
      [60, 10],
      [10, 10],
    ],
  };

  assert.throws(() => resizeClosedStrokeBounds(openStroke, 100, 100), { message: 'Closed stroke required.' });
  assert.throws(() => resizeClosedStrokeBounds(openStroke, '', 100), { message: 'Closed stroke required.' });
  assert.throws(() => resizeClosedStrokeBounds({ ...closedStroke, points: [[10, 10], [Infinity, 20], [10, 10]] }, 100, 100), {
    message: 'Invalid sketch point.',
  });
  assert.throws(
    () => resizeClosedStrokeBounds({ ...closedStroke, points: [[10, 10], [Infinity, 20], [10, 10]] }, '', 100),
    { message: 'Invalid sketch point.' },
  );
  assert.throws(() => resizeClosedStrokeBounds(closedStroke, '', 100), { message: 'Invalid sketch dimension.' });
  assert.throws(() => resizeClosedStrokeBounds(closedStroke, 0, 100), { message: 'Sketch dimension must be positive.' });
  assert.throws(() => resizeClosedStrokeBounds(flatStroke, 100, 100), {
    message: 'Sketch profile bounds invalid.',
  });
});

test('assertLockedDimensionsPreserved blocks width or height changes for locked profiles', () => {
  const lockedStroke: SketchStroke = {
    ...closedQuadStroke,
    dimensionLocks: { width: true, height: true },
  };

  assert.doesNotThrow(() => assertLockedDimensionsPreserved(lockedStroke, { ...lockedStroke, points: closedQuadStroke.points }));
  assert.throws(
    () =>
      assertLockedDimensionsPreserved(lockedStroke, {
        ...lockedStroke,
        points: [
          [10, 10],
          [80, 10],
          [80, 60],
          [10, 60],
          [10, 10],
        ],
      }),
    { message: 'Locked sketch dimension would change.' },
  );
  assert.throws(
    () =>
      assertLockedDimensionsPreserved(lockedStroke, {
        ...lockedStroke,
        points: [
          [10, 10],
          [60, 10],
          [60, 90],
          [10, 90],
          [10, 10],
        ],
      }),
    { message: 'Locked sketch dimension would change.' },
  );
});

test('assertLockedDimensionsPreserved allows unlocked dimension changes', () => {
  assert.doesNotThrow(() =>
    assertLockedDimensionsPreserved(
      closedQuadStroke,
      {
        ...closedQuadStroke,
        points: [
          [10, 10],
          [80, 10],
          [80, 90],
          [10, 90],
          [10, 10],
        ],
      },
    ),
  );
});

test('logicalPointCount excludes duplicate closing point from closed strokes', () => {
  assert.equal(logicalPointCount(closedStroke), 3);
  assert.equal(logicalPointCount(openStroke), 3);
});

test('deleteClosedStrokePoint removes a logical point and keeps the closing duplicate', () => {
  const next = deleteClosedStrokePoint(closedQuadStroke, 1);

  assert.notEqual(next, closedQuadStroke);
  assert.notEqual(next.points, closedQuadStroke.points);
  assert.notEqual(next.points[0], closedQuadStroke.points[0]);
  assert.notEqual(next.points[3], closedQuadStroke.points[4]);
  assert.deepEqual(next, {
    primitiveId: 'primitive-front-quad',
    view: 'front',
    points: [
      [10, 10],
      [60, 60],
      [10, 60],
      [10, 10],
    ],
    closed: true,
  });
});

test('deleteClosedStrokePoint normalizes the final closing point to the first logical point', () => {
  const next = deleteClosedStrokePoint(closedQuadStroke, 4);

  assert.deepEqual(next.points, [
    [60, 10],
    [60, 60],
    [10, 60],
    [60, 10],
  ]);
});

test('deleteClosedStrokePoint rejects invalid states with exact errors', () => {
  const triangleStroke: SketchStroke = {
    ...closedStroke,
    points: [
      [10, 10],
      [60, 10],
      [60, 60],
      [10, 10],
    ],
  };

  assert.throws(() => deleteClosedStrokePoint(openStroke, 1), { message: 'Closed stroke required.' });
  assert.throws(() => deleteClosedStrokePoint(closedStroke, 9), { message: 'Invalid sketch point index.' });
  assert.throws(() => deleteClosedStrokePoint(triangleStroke, 1), {
    message: 'Closed profile needs at least 3 points.',
  });
});

test('hitTestSketchPoint returns the closest editable point in the requested view', () => {
  const strokes: SketchStroke[] = [
    openStroke,
    {
      primitiveId: 'primitive-top-1',
      view: 'top',
      points: [
        [10, 10],
        [90, 10],
        [90, 90],
        [10, 10],
      ],
      closed: true,
    },
    {
      primitiveId: 'primitive-front-3',
      view: 'front',
      points: [
        [20, 20],
        [50, 50],
        [20, 20],
      ],
      closed: true,
    },
  ];

  assert.deepEqual(hitTestSketchPoint(strokes, 'front', [21, 19], 4), { strokeIndex: 2, pointIndex: 0 });
  assert.deepEqual(hitTestSketchPoint(strokes, 'top', [12, 12], 5), { strokeIndex: 1, pointIndex: 0 });
  assert.equal(hitTestSketchPoint(strokes, 'side', [12, 12], 5), null);
});
