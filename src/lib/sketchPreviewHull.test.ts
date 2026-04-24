import assert from 'node:assert/strict';
import test from 'node:test';

import { buildSketchPreviewHullRequest, shouldUseSketchPreviewHull } from './sketchPreviewHull';
import type { SketchStroke } from './sketchWorkspaceState';

const front: SketchStroke = {
  primitiveId: 'front-box',
  view: 'front',
  closed: true,
  points: [
    [10, 20],
    [60, 20],
    [60, 50],
    [10, 50],
    [10, 20],
  ],
};

const top: SketchStroke = {
  primitiveId: 'top-footprint',
  view: 'top',
  closed: true,
  points: [
    [10, 5],
    [60, 5],
    [60, 27],
    [10, 27],
    [10, 5],
  ],
};

const side: SketchStroke = {
  primitiveId: 'side-footprint',
  view: 'side',
  closed: true,
  points: [
    [5, 20],
    [27, 20],
    [27, 50],
    [5, 50],
    [5, 20],
  ],
};

test('shouldUseSketchPreviewHull requires Front plus Top or Side closed profiles', () => {
  assert.equal(shouldUseSketchPreviewHull([front]), false);
  assert.equal(shouldUseSketchPreviewHull([top, side]), false);
  assert.equal(shouldUseSketchPreviewHull([front, top]), true);
  assert.equal(shouldUseSketchPreviewHull([front, side]), true);
});

test('buildSketchPreviewHullRequest carries all orthographic SketchDocument views and constrained depth', () => {
  const request = buildSketchPreviewHullRequest([front, top, side]);

  assert.ok(!('error' in request));
  assert.equal(request.partId, 'sketch-preview-hull');
  assert.equal(request.fallbackDepth, 22);
  assert.deepEqual(request.document.sketches?.map((sketch) => sketch.view), ['front', 'top', 'side']);
  assert.equal(request.document.sketches?.[0]?.primitives?.[0]?.primitiveId, 'front-box');
  assert.equal(request.document.sketches?.[1]?.primitives?.[0]?.primitiveId, 'top-footprint');
  assert.equal(request.document.sketches?.[2]?.primitives?.[0]?.primitiveId, 'side-footprint');
});
