import assert from 'node:assert/strict';
import test from 'node:test';

import { buildSketchProjections } from './sketchProjection';
import type { SketchStroke } from './sketchWorkspaceState';

const frontRectangle: SketchStroke = {
  primitiveId: 'primitive-front-1',
  view: 'front',
  points: [
    [20, 20],
    [60, 20],
    [60, 60],
    [20, 60],
    [20, 20],
  ],
  closed: true,
};

test('buildSketchProjections keeps front rectangle as source profile and derives extrusion depth views', () => {
  const projections = buildSketchProjections(frontRectangle, 12);

  assert.equal(projections.length, 3);

  const front = projections.find((projection) => projection.view === 'front');
  assert.ok(front);
  assert.equal(front.label, 'FRONT / SOURCE PROFILE');
  assert.equal(front.role, 'source');
  assert.deepEqual(front.points, frontRectangle.points);
  assert.equal(front.path, 'M20 20 L60 20 L60 60 L20 60 Z');
  assert.match(front.explanation, /original closed profile/i);

  const top = projections.find((projection) => projection.view === 'top');
  assert.ok(top);
  assert.equal(top.label, 'TOP / EXTRUSION DEPTH');
  assert.equal(top.role, 'derived');
  assert.deepEqual(top.bounds, { left: 20, top: 0, width: 40, height: 12, depth: 12 });
  assert.match(top.explanation, /12mm/i);

  const side = projections.find((projection) => projection.view === 'side');
  assert.ok(side);
  assert.equal(side.label, 'SIDE / EXTRUSION DEPTH');
  assert.equal(side.role, 'derived');
  assert.deepEqual(side.bounds, { left: 0, top: 20, width: 12, height: 40, depth: 12 });
  assert.match(side.explanation, /12mm/i);
});
