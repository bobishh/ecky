import assert from 'node:assert/strict';
import test from 'node:test';

import { buildSketchLearningLens, extrudeLearningLens } from './sketchLearningLens';

test('extrudeLearningLens explains 12mm extrusion math copy', () => {
  const lens = extrudeLearningLens(12);

  assert.equal(lens.title, 'LEARNING LENS / MATH LENS');
  assert.equal(lens.operationLabel, 'EXTRUDE 12MM');
  assert.equal(lens.formula, '(x, y) -> (x, y, z)');
  assert.equal(lens.domain, '0 <= z <= 12');
  assert.match(lens.explanation, /closed 2D profile/);
  assert.match(lens.explanation, /12mm of depth/);
});

test('buildSketchLearningLens explains BRep auto snap repair math when present', () => {
  const lens = buildSketchLearningLens(22, [
    {
      action: 'AUTO SNAP',
      primitiveId: 'primitive-front',
      detail: 'BREP AUTO SNAP FRONT primitive-front bounds 50x30 -> 80x40',
    },
  ]);

  assert.equal(lens.operationLabel, 'BREP AUTO SNAP');
  assert.match(lens.explanation, /primitive-front/);
  assert.match(lens.explanation, /exact BRep hidden-line bounds/);
  assert.match(lens.formula, /x'/);
  assert.match(lens.formula, /brepWidth/);
  assert.match(lens.domain, /50x30 -> 80x40/);
});

test('buildSketchLearningLens explains BRep auto contain repair math when present', () => {
  const lens = buildSketchLearningLens(22, [
    {
      action: 'AUTO SNAP',
      primitiveId: 'primitive-front',
      detail: 'BREP AUTO CONTAIN FRONT primitive-front bounds 50x30 -> 54.2x30',
    },
  ]);

  assert.equal(lens.operationLabel, 'BREP AUTO CONTAIN');
  assert.match(lens.explanation, /expanded primitive-front source bounds/);
  assert.match(lens.explanation, /contain exact BRep hidden-line bounds/);
  assert.match(lens.formula, /x'/);
  assert.match(lens.domain, /50x30 -> 54\.2x30/);
});

test('buildSketchLearningLens explains BRep topology redraw as projection-derived seed', () => {
  const lens = buildSketchLearningLens(22, [
    {
      action: 'TOPOLOGY REDRAW',
      primitiveId: 'primitive-front',
      detail: 'TOPOLOGY REDRAW FRONT primitive-front / derived from BRep projection; not authoring history',
    },
  ]);

  assert.equal(lens.operationLabel, 'BREP TOPOLOGY REDRAW');
  assert.match(lens.explanation, /primitive-front/);
  assert.match(lens.explanation, /projection-derived/i);
  assert.match(lens.explanation, /not original authoring history/i);
  assert.equal(lens.formula, 'BRep HLR loop -> Sketch polyline');
  assert.match(lens.domain, /derived from BRep projection/);
});
