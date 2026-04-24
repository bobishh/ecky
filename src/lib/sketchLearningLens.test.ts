import assert from 'node:assert/strict';
import test from 'node:test';

import { extrudeLearningLens } from './sketchLearningLens';

test('extrudeLearningLens explains 12mm extrusion math copy', () => {
  const lens = extrudeLearningLens(12);

  assert.equal(lens.title, 'LEARNING LENS / MATH LENS');
  assert.equal(lens.operationLabel, 'EXTRUDE 12MM');
  assert.equal(lens.formula, '(x, y) -> (x, y, z)');
  assert.equal(lens.domain, '0 <= z <= 12');
  assert.match(lens.explanation, /closed 2D profile/);
  assert.match(lens.explanation, /12mm of depth/);
});
