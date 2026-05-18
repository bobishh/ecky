import assert from 'node:assert/strict';
import test from 'node:test';

import { shouldPreserveWorkingCopyMacroDraft } from './manualController';

test('param commit keeps current macro code draft when committed macro differs', () => {
  const preserve = shouldPreserveWorkingCopyMacroDraft(
    { macroCode: 'draft macro();', dirty: true },
    'committed macro();',
  );

  assert.equal(preserve, true);
});

test('param commit does not preserve macro draft when working copy matches commit', () => {
  const preserve = shouldPreserveWorkingCopyMacroDraft(
    { macroCode: 'macro();', dirty: false },
    'macro();',
  );

  assert.equal(preserve, false);
});
