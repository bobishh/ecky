import assert from 'node:assert/strict';
import test from 'node:test';

import { get } from 'svelte/store';

import {
  enqueueAgentTerminalSnapshot,
  resetAgentTerminalStore,
  setAgentTerminalSelection,
  visibleAgentTerminalStore,
} from './agentTerminalStore';

test('agentTerminalStore keeps separate sessions for the same agent and prefers the selected session', async () => {
  resetAgentTerminalStore();
  enqueueAgentTerminalSnapshot({
    agentId: 'gemini',
    agentLabel: 'Gemini',
    sessionId: 'session-a',
    sessionNonce: 1,
    screenText: 'session a',
    vtStream: 'session a',
    vtDelta: null,
    attentionRequired: false,
    summary: null,
    active: true,
    updatedAt: 10,
  });
  enqueueAgentTerminalSnapshot({
    agentId: 'gemini',
    agentLabel: 'Gemini',
    sessionId: 'session-b',
    sessionNonce: 2,
    screenText: 'session b',
    vtStream: 'session b',
    vtDelta: null,
    attentionRequired: false,
    summary: null,
    active: true,
    updatedAt: 20,
  });

  await new Promise((resolve) => setTimeout(resolve, 90));

  setAgentTerminalSelection('gemini', 'session-a');
  assert.equal(get(visibleAgentTerminalStore)?.sessionId, 'session-a');

  setAgentTerminalSelection('gemini', 'session-b');
  assert.equal(get(visibleAgentTerminalStore)?.sessionId, 'session-b');

  resetAgentTerminalStore();
});
