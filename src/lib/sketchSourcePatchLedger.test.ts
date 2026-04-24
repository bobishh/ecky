import assert from 'node:assert/strict';
import test from 'node:test';

import { appendSketchSourcePatch, compactRepairDetail } from './sketchSourcePatchLedger';

test('appendSketchSourcePatch appends deterministic patch ids and preserves evidence', () => {
  const entries = appendSketchSourcePatch([], {
    action: 'CLEAN UP',
    primitiveId: 'primitive-front-1',
    detail: 'primitive-front-1 cleaned to rectangle width 47mm height 34mm.',
  });

  assert.deepEqual(
    appendSketchSourcePatch(entries, {
      action: 'REPAIR IMPORT',
      primitiveId: 'primitive-front-1',
      detail: "sketch 'sketch-front' primitive 'primitive-front-1' width dimension repaired 99mm -> 47mm.",
    }),
    [
      {
        patchId: 'source-patch-1',
        action: 'CLEAN UP',
        primitiveId: 'primitive-front-1',
        detail: 'primitive-front-1 cleaned to rectangle width 47mm height 34mm.',
      },
      {
        patchId: 'source-patch-2',
        action: 'REPAIR IMPORT',
        primitiveId: 'primitive-front-1',
        detail: "sketch 'sketch-front' primitive 'primitive-front-1' width dimension repaired 99mm -> 47mm.",
      },
    ],
  );
});

test('compactRepairDetail strips UI prefix only', () => {
  assert.equal(
    compactRepairDetail("REPAIR AVAILABLE / sketch 'sketch-front' primitive 'primitive-front-1' width dimension repaired 99mm -> 47mm."),
    "sketch 'sketch-front' primitive 'primitive-front-1' width dimension repaired 99mm -> 47mm.",
  );
  assert.equal(compactRepairDetail('no prefix'), 'no prefix');
});
