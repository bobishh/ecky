import test from 'node:test';
import assert from 'node:assert/strict';

import {
  pointerMovedBeyondClickThreshold,
  shouldHandleSelectionClick,
  shouldHandleViewerClick,
} from './viewerInteraction';

test('pointerMovedBeyondClickThreshold treats tiny pointer drift as click intent', () => {
  assert.equal(pointerMovedBeyondClickThreshold({ x: 10, y: 20 }, { x: 13, y: 23 }), false);
});

test('pointerMovedBeyondClickThreshold treats orbit drag as non-click intent', () => {
  assert.equal(pointerMovedBeyondClickThreshold({ x: 10, y: 20 }, { x: 16, y: 21 }), true);
  assert.equal(pointerMovedBeyondClickThreshold({ x: 10, y: 20 }, { x: 12, y: 25 }), true);
});

test('shouldHandleSelectionClick requires selection mode and click-distance movement', () => {
  assert.equal(
    shouldHandleSelectionClick({
      hideModelWhileBusy: false,
      selectionMode: false,
      pointerDownAt: { x: 10, y: 10 },
      current: { x: 10, y: 10 },
    }),
    false,
  );
  assert.equal(
    shouldHandleSelectionClick({
      hideModelWhileBusy: false,
      selectionMode: true,
      pointerDownAt: { x: 10, y: 10 },
      current: { x: 18, y: 10 },
    }),
    false,
  );
  assert.equal(
    shouldHandleSelectionClick({
      hideModelWhileBusy: false,
      selectionMode: true,
      pointerDownAt: { x: 10, y: 10 },
      current: { x: 12, y: 12 },
    }),
    true,
  );
});

test('shouldHandleViewerClick allows inspect clicks without selection mode but rejects drags', () => {
  assert.equal(
    shouldHandleViewerClick({
      hideModelWhileBusy: false,
      pointerDownAt: { x: 10, y: 10 },
      current: { x: 12, y: 12 },
    }),
    true,
  );
  assert.equal(
    shouldHandleViewerClick({
      hideModelWhileBusy: false,
      pointerDownAt: { x: 10, y: 10 },
      current: { x: 18, y: 10 },
    }),
    false,
  );
});
