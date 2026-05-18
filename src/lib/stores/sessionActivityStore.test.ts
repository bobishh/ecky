import assert from 'node:assert/strict';
import test from 'node:test';

import {
  clearSessionActivityEvents,
  currentSessionActivityEvents,
  recordSessionActivityEvent,
} from './sessionActivityStore';

test('recordSessionActivityEvent appends stable session event defaults', () => {
  clearSessionActivityEvents();

  const event = recordSessionActivityEvent({
    threadId: 'thread-1',
    versionId: 'version-1',
    kind: 'render_started',
    title: 'Render started',
    summary: 'Rendering current draft.',
    severity: 'info',
  });

  const events = currentSessionActivityEvents();
  assert.equal(events.length, 1);
  assert.equal(events[0].id, event.id);
  assert.equal(events[0].sessionId, 'local-session');
  assert.deepEqual(events[0].actor, { kind: 'system', id: 'ecky' });

  clearSessionActivityEvents();
});
