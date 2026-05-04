import assert from 'node:assert/strict';
import test from 'node:test';
import { get } from 'svelte/store';

import { requestQueue } from './requestQueue';
import { session, setManualRenderActive } from './sessionStore';

test('manual render cannot move startup session out of booting phase', () => {
  requestQueue.clear();
  setManualRenderActive(false);
  session.setPhase('booting');

  setManualRenderActive(true, { threadId: 'thread-1', messageId: 'msg-1' });

  const current = get(session);
  assert.equal(current.phase, 'booting');
  assert.equal(current.isManual, true);

  setManualRenderActive(false);
  session.setPhase('idle');
  requestQueue.clear();
});
