import assert from 'node:assert/strict';
import test from 'node:test';
import { codexQueueErrorCopy } from './providerQueuePresentation';

test('queued Codex writer contention shows automatic wait, while terminal failures retain raw error', () => {
  const raw = 'thread codex-7 already has an active writer';
  assert.equal(
    codexQueueErrorCopy('queued', raw),
    'Codex is busy. This request will send automatically when ready.',
  );
  assert.equal(codexQueueErrorCopy('failed', raw), raw);
  assert.equal(codexQueueErrorCopy('queued', 'workspace sandbox denied write'), 'workspace sandbox denied write');
  assert.equal(codexQueueErrorCopy('queued', null), null);
});
