import assert from 'node:assert/strict';
import test from 'node:test';

import {
  deriveMascotStateForThreadAgent,
  derivePrimaryAgentLabel,
  derivePrimaryAgentId,
  hasLiveAgentSession,
  normalizeMcpMode,
  phaseLabelForThreadAgentState,
  promptBelongsToPrimaryAgent,
  deriveThreadAttentionIds,
  resolveActivePendingPrompt,
  shouldAutoFocusAgentWorkingVersion,
  usesMcpConnection,
  usesActiveMcpMode,
} from './state';

test('normalizeMcpMode falls back to active when legacy auto-agents exist', () => {
  assert.equal(
    normalizeMcpMode(undefined, [
      { id: 'a1', label: 'Primary', cmd: 'codex', args: [], enabled: true },
    ]),
    'active',
  );
  assert.equal(normalizeMcpMode(undefined, []), 'passive');
});

test('derivePrimaryAgentId keeps a valid enabled primary and otherwise picks the first enabled agent', () => {
  const agents = [
    { id: 'a1', label: 'A1', cmd: 'codex', args: [], enabled: false },
    { id: 'a2', label: 'A2', cmd: 'codex', args: [], enabled: true },
    { id: 'a3', label: 'A3', cmd: 'codex', args: [], enabled: true },
  ];
  assert.equal(derivePrimaryAgentId(agents, 'a3'), 'a3');
  assert.equal(derivePrimaryAgentId(agents, 'a1'), 'a2');
  assert.equal(derivePrimaryAgentId([], null), null);
});

test('derivePrimaryAgentLabel resolves the enabled primary agent label', () => {
  const agents = [
    { id: 'a1', label: 'Alpha', cmd: 'codex', args: [], enabled: true },
    { id: 'a2', label: 'Beta', cmd: 'gemini', args: [], enabled: true },
  ];

  assert.equal(derivePrimaryAgentLabel(agents, 'a2'), 'Beta');
  assert.equal(derivePrimaryAgentLabel(agents, 'missing'), 'Alpha');
});

test('promptBelongsToPrimaryAgent ignores stale prompts from a non-primary agent', () => {
  const agents = [
    { id: 'a1', label: 'Alpha', cmd: 'codex', args: [], enabled: true },
    { id: 'a2', label: 'Beta', cmd: 'gemini', args: [], enabled: true },
  ];

  assert.equal(promptBelongsToPrimaryAgent(agents, 'a2', 'Beta'), true);
  assert.equal(promptBelongsToPrimaryAgent(agents, 'a2', 'Alpha'), false);
});

test('resolveActivePendingPrompt only activates a prompt bound to the current thread', () => {
  assert.equal(
    resolveActivePendingPrompt({
      prompts: [
        { requestId: 'foreign', agentLabel: 'Alpha', threadId: 'thread-foreign' },
        { requestId: 'current', agentLabel: 'Alpha', threadId: 'thread-current' },
      ],
      currentThreadId: 'thread-current',
      connectionType: 'mcp',
      mode: 'passive',
      autoAgents: [],
      primaryAgentId: null,
    })?.requestId,
    'current',
  );

  assert.equal(
    resolveActivePendingPrompt({
      prompts: [{ requestId: 'foreign', agentLabel: 'Alpha', threadId: 'thread-foreign' }],
      currentThreadId: 'thread-current',
      connectionType: 'mcp',
      mode: 'passive',
      autoAgents: [],
      primaryAgentId: null,
    }),
    null,
  );
});

test('resolveActivePendingPrompt ignores stale prompts from non-primary agents in active mode', () => {
  assert.equal(
    resolveActivePendingPrompt({
      prompts: [
        { requestId: 'alpha', agentLabel: 'Alpha', threadId: 'thread-1' },
        { requestId: 'beta', agentLabel: 'Beta', threadId: 'thread-1' },
      ],
      currentThreadId: 'thread-1',
      connectionType: 'mcp',
      mode: 'active',
      autoAgents: [
        { id: 'a1', label: 'Alpha', cmd: 'codex', args: [], enabled: true },
        { id: 'a2', label: 'Beta', cmd: 'codex', args: [], enabled: true },
      ],
      primaryAgentId: 'a2',
    })?.requestId,
    'beta',
  );
});

test('deriveThreadAttentionIds marks only foreign-thread prompt and screenshot requests', () => {
  assert.deepEqual(
    deriveThreadAttentionIds({
      prompts: [
        { requestId: 'current', agentLabel: 'Alpha', threadId: 'thread-current' },
        { requestId: 'foreign', agentLabel: 'Alpha', threadId: 'thread-foreign' },
      ],
      screenshots: [
        { requestId: 'shot-current', threadId: 'thread-current' },
        { requestId: 'shot-other', threadId: 'thread-other' },
      ],
      activePromptRequestId: 'current',
      currentThreadId: 'thread-current',
    }).sort(),
    ['thread-foreign', 'thread-other'],
  );
});

test('usesActiveMcpMode only enables in-app MCP routing for active mode', () => {
  assert.equal(usesActiveMcpMode('mcp', 'active'), true);
  assert.equal(usesActiveMcpMode('mcp', 'passive'), false);
  assert.equal(usesActiveMcpMode('api_key', 'active'), false);
});

test('usesMcpConnection enables queued dialogue for both active and passive MCP', () => {
  assert.equal(usesMcpConnection('mcp'), true);
  assert.equal(usesMcpConnection('api_key'), false);
  assert.equal(usesMcpConnection(null), false);
});

test('hasLiveAgentSession treats any live MCP session as a connected workspace agent', () => {
  assert.equal(hasLiveAgentSession([]), false);
  assert.equal(
    hasLiveAgentSession([
      {
        sessionId: 'session-1',
        clientKind: 'mcp-http',
        hostLabel: 'Gemini CLI',
        agentLabel: 'Gemini',
        llmModelId: null,
        llmModelLabel: null,
        threadId: null,
        messageId: null,
        modelId: null,
        phase: 'idle',
        statusText: 'Agent joined the workspace.',
        updatedAt: 1,
      },
    ]),
    true,
  );
});

test('shouldAutoFocusAgentWorkingVersion only follows updates inside the active workbench thread', () => {
  assert.equal(
    shouldAutoFocusAgentWorkingVersion({
      currentView: 'workbench',
      activeThreadId: 'thread-1',
      eventThreadId: 'thread-1',
    }),
    true,
  );

  assert.equal(
    shouldAutoFocusAgentWorkingVersion({
      currentView: 'workbench',
      activeThreadId: 'thread-2',
      eventThreadId: 'thread-1',
    }),
    false,
  );

  assert.equal(
    shouldAutoFocusAgentWorkingVersion({
      currentView: 'inventory',
      activeThreadId: 'thread-1',
      eventThreadId: 'thread-1',
    }),
    false,
  );

  assert.equal(
    shouldAutoFocusAgentWorkingVersion({
      currentView: 'workbench',
      activeThreadId: null,
      eventThreadId: 'thread-1',
    }),
    false,
  );
});

test('phaseLabelForThreadAgentState handles waiting-for-user explicitly', () => {
  assert.equal(
    phaseLabelForThreadAgentState({
      connectionState: 'waiting',
      agentLabel: 'Primary',
      llmModelLabel: null,
      phase: 'waiting_for_user',
      statusText: null,
      updatedAt: 1,
    }),
    'waiting for your next message...',
  );
});

test('deriveMascotStateForThreadAgent maps waking and disconnected backend states to strict connection semantics', () => {
  assert.deepEqual(
    deriveMascotStateForThreadAgent({
      connectionState: 'waking',
      agentLabel: 'Primary',
      llmModelLabel: null,
      phase: null,
      statusText: 'Waking Primary...',
      updatedAt: 1,
    }),
    {
      connected: true,
      mode: 'waking',
      bubble: 'Waking Primary...',
    },
  );

  assert.deepEqual(
    deriveMascotStateForThreadAgent({
      connectionState: 'disconnected',
      agentLabel: 'Primary',
      llmModelLabel: null,
      phase: null,
      statusText: null,
      updatedAt: 1,
    }),
    {
      connected: false,
      mode: 'idle',
      bubble: 'Primary disconnected.',
    },
  );
});

test('deriveMascotStateForThreadAgent keeps sleeping agents silent', () => {
  assert.deepEqual(
    deriveMascotStateForThreadAgent({
      connectionState: 'sleeping',
      agentLabel: 'Claude',
      llmModelLabel: null,
      phase: null,
      statusText: null,
      updatedAt: 1,
    }),
    {
      connected: false,
      mode: 'idle',
      bubble: '',
    },
  );
});

test('deriveMascotStateForThreadAgent treats connected-but-idle active sessions as light instead of thinking', () => {
  assert.deepEqual(
    deriveMascotStateForThreadAgent({
      connectionState: 'active',
      agentLabel: 'Gemini',
      llmModelLabel: null,
      providerKind: 'gemini',
      sessionId: 'session-1',
      phase: 'idle',
      statusText: 'Connected to Ecky.',
      busy: false,
      activityLabel: null,
      activityStartedAt: null,
      attentionKind: null,
      waitingOnPrompt: false,
      updatedAt: 1,
    }),
    {
      connected: true,
      mode: 'light',
      bubble: 'Connected to Ecky.',
    },
  );
});
