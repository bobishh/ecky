import assert from 'node:assert/strict';
import test from 'node:test';

import { autoRepairOrthographicSketchStrokes } from './sketchOrthographicRepair';
import { buildSketchDraftRequest } from './sketchWorkspaceState';
import { closedStrokeBounds } from './sketchEditState';
import type { SketchStroke } from './sketchWorkspaceState';

const frontStroke: SketchStroke = {
  primitiveId: 'primitive-front-wide',
  view: 'front',
  points: [
    [10, 20],
    [60, 20],
    [60, 50],
    [10, 50],
    [10, 20],
  ],
  closed: true,
};

test('autoRepairOrthographicSketchStrokes snaps top width to front width and draft can build', () => {
  const topStroke: SketchStroke = {
    primitiveId: 'primitive-top-narrow',
    view: 'top',
    points: [
      [10, 10],
      [50, 10],
      [50, 32],
      [10, 32],
      [10, 10],
    ],
    closed: true,
  };

  const initial = buildSketchDraftRequest([frontStroke, topStroke]);
  assert.ok('error' in initial);
  assert.equal(initial.error, 'Top view width 40mm must match Front view width 50mm.');

  const result = autoRepairOrthographicSketchStrokes([frontStroke, topStroke]);
  assert.deepEqual(result.repairs.map((repair) => repair.detail), [
    'TOP X 40MM -> 50MM',
    'TOP X RANGE 5..55MM -> 10..60MM',
  ]);
  assert.equal(closedStrokeBounds(result.strokes[1]).width, 50);
  assert.equal(closedStrokeBounds(result.strokes[1]).height, 22);
  assert.deepEqual(result.strokes[1].points, [
    [10, 10],
    [60, 10],
    [60, 32],
    [10, 32],
    [10, 10],
  ]);

  const repaired = buildSketchDraftRequest(result.strokes);
  assert.ok(!('error' in repaired));
  assert.equal(repaired.amount, 22);
});

test('autoRepairOrthographicSketchStrokes aligns top x range to front x range', () => {
  const shiftedTopStroke: SketchStroke = {
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
  };

  const initial = buildSketchDraftRequest([frontStroke, shiftedTopStroke]);
  assert.ok('error' in initial);
  assert.equal(initial.error, 'Top view x range 30..80mm must match Front view x range 10..60mm.');

  const result = autoRepairOrthographicSketchStrokes([frontStroke, shiftedTopStroke]);
  assert.deepEqual(result.repairs.map((repair) => repair.detail), [
    'TOP X RANGE 30..80MM -> 10..60MM',
  ]);
  assert.deepEqual(result.strokes[1].points, [
    [10, 10],
    [60, 10],
    [60, 32],
    [10, 32],
    [10, 10],
  ]);

  const repaired = buildSketchDraftRequest(result.strokes);
  assert.ok(!('error' in repaired));
  assert.equal(repaired.amount, 22);
});

test('autoRepairOrthographicSketchStrokes applies chained side height and depth snaps', () => {
  const topStroke: SketchStroke = {
    primitiveId: 'primitive-top',
    view: 'top',
    points: [
      [10, 10],
      [60, 10],
      [60, 32],
      [10, 32],
      [10, 10],
    ],
    closed: true,
  };
  const sideStroke: SketchStroke = {
    primitiveId: 'primitive-side',
    view: 'side',
    points: [
      [10, 10],
      [40, 10],
      [40, 35],
      [10, 35],
      [10, 10],
    ],
    closed: true,
  };

  const result = autoRepairOrthographicSketchStrokes([frontStroke, topStroke, sideStroke]);
  assert.deepEqual(result.repairs.map((repair) => repair.detail), [
    'SIDE Y 25MM -> 30MM',
    'SIDE Y RANGE 7.5..37.5MM -> 20..50MM',
    'SIDE X 30MM -> 22MM',
    'SIDE X RANGE 14..36MM -> 10..32MM',
  ]);
  assert.equal(closedStrokeBounds(result.strokes[2]).height, 30);
  assert.equal(closedStrokeBounds(result.strokes[2]).width, 22);

  const repaired = buildSketchDraftRequest(result.strokes);
  assert.ok(!('error' in repaired));
  assert.equal(repaired.amount, 22);
});
