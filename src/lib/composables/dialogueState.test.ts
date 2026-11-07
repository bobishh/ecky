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

test('deriveDialogueState routes Codex provider mode before API or MCP without requiring a binding', () => {
  assert.deepEqual(
    deriveDialogueState(
      {
        requestId: 'req-from-other-runtime',
        agentLabel: 'Other agent',
      },
      true,
      'provider:codex',
      null,
    ),
    {
      mode: 'provider',
      providerId: 'codex',
      externalConversationId: null,
      label: 'Codex',
      supportsSteer: true,
      supportsStop: true,
    },
  );
});

test('deriveDialogueState exposes exact owned Codex id after binding loads', () => {
  assert.deepEqual(
    deriveDialogueState(
      null,
      false,
      'provider:codex',
      {
        codexThreadId: 'codex-thread-7',
        label: 'Gearbox agent',
      },
    ),
    {
      mode: 'provider',
      providerId: 'codex',
      externalConversationId: 'codex-thread-7',
      label: 'Codex',
      supportsSteer: true,
      supportsStop: true,
    },
  );
});

test('deriveDialogueState labels Grok and keeps steer off until the snapshot says otherwise', () => {
  assert.deepEqual(
    deriveDialogueState(null, false, 'provider:grok', {
      providerId: 'grok',
      externalConversationId: 'grok-session-7',
      label: 'Grok',
      supportsSteer: false,
      supportsStop: true,
    }),
    {
      mode: 'provider',
      providerId: 'grok',
      externalConversationId: 'grok-session-7',
      label: 'Grok',
      supportsSteer: false,
      supportsStop: true,
    },
  );
});

test('deriveDialogueState exposes Agy capabilities without inventing steer', () => {
  assert.deepEqual(
    deriveDialogueState(null, false, 'provider:agy', {
      providerId: 'agy',
      externalConversationId: 'agy-conversation-7',
      label: 'Agy',
      supportsSteer: false,
      supportsStop: true,
    }),
    {
      mode: 'provider',
      providerId: 'agy',
      externalConversationId: 'agy-conversation-7',
      label: 'Agy',
      supportsSteer: false,
      supportsStop: true,
    },
  );
});
