import assert from 'node:assert/strict';
import test from 'node:test';

import { loadBoundCodeReference, resolveCodeModalSource } from './codeModalSource';

test('bound code reference preserves backend failure before returning any source', async () => {
  const rawError = new Error('project source missing (raw backend body)');
  await assert.rejects(
    loadBoundCodeReference('/project/model.ecky', async () => { throw rawError; }),
    (error) => error === rawError,
  );
});

test('bound code reference rejects path drift and returns only the exact bound source', async () => {
  await assert.rejects(
    loadBoundCodeReference('/old/model.ecky', async () => ({ file: '/current/model.ecky', source: '(model)' })),
    /Referenced: \/old\/model\.ecky\. Current: \/current\/model\.ecky/,
  );
  const source = { file: '/current/model.ecky', source: '(model)' };
  assert.equal(await loadBoundCodeReference(source.file, async () => source), source);
});

test('active viewport render source wins over bound project source', () => {
  assert.deepEqual(
    resolveCodeModalSource({
      activeRenderSource: 'print("agent draft")',
      boundSource: 'print("committed")',
      activeRenderMatchesViewport: true,
    }),
    {
      source: 'print("agent draft")',
      authority: 'draft',
    },
  );
});

test('bound project source remains authoritative without an active render draft', () => {
  assert.deepEqual(
    resolveCodeModalSource({
      activeRenderSource: 'print("working copy")',
      boundSource: 'print("committed")',
      activeRenderMatchesViewport: false,
    }),
    {
      source: 'print("committed")',
      authority: 'bound',
    },
  );
});
