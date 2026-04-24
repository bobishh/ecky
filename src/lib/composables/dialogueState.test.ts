import assert from 'node:assert/strict';
import test from 'node:test';
import { deriveDialogueState } from './dialogueState';

test('deriveDialogueState prefers pending agent reply over queued dialogue mode', () => {
  assert.deepEqual(
    deriveDialogueState(
      {
        requestId: 'req-1',
        agentLabel: 'Codex',
      },
      true,
    ),
    {
      mode: 'agent-reply',
      requestId: 'req-1',
      agentLabel: 'Codex',
    },
  );
});

test('deriveDialogueState returns mcp-idle when queued dialogue is enabled without pending prompt', () => {
  assert.deepEqual(deriveDialogueState(null, true), { mode: 'mcp-idle' });
});

test('deriveDialogueState falls back to generate mode', () => {
  assert.deepEqual(deriveDialogueState(null, false), { mode: 'generate' });
});
