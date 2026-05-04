import assert from 'node:assert/strict';
import test from 'node:test';

import { fitRectToViewport } from './windowGeometry';

test('fitRectToViewport shrinks oversized rect and clamps it fully inside viewport', () => {
  assert.deepEqual(
    fitRectToViewport(
      { x: 140, y: 120, width: 1000, height: 700 },
      { width: 400, height: 300 },
      { width: 900, height: 620 },
    ),
    { x: 0, y: 0, width: 900, height: 620 },
  );
});

test('fitRectToViewport keeps size when already valid and only repositions offscreen rect', () => {
  assert.deepEqual(
    fitRectToViewport(
      { x: 320, y: 560, width: 980, height: 260 },
      { width: 350, height: 260 },
      { width: 1180, height: 680 },
    ),
    { x: 200, y: 420, width: 980, height: 260 },
  );
});
