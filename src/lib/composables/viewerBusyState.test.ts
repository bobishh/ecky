import assert from 'node:assert/strict';
import test from 'node:test';
import { deriveViewerBusyState } from './viewerBusyState';
import type { AgentSession, Request } from '../types/domain';
import type { ThreadAgentState } from '../tauri/client';

function request(overrides: Partial<Request>): Request {
  return {
    id: 'req',
    prompt: '',
    attachments: [],
    createdAt: 1,
    phase: 'generating',
    attempt: 1,
    maxAttempts: 1,
    maxVerifyAttempts: 0,
    isQuestion: false,
    lightResponse: '',
    screenshot: null,
    threadId: 'thread-1',
    baseMessageId: 'msg-1',
    baseModelId: 'model-1',
    result: null,
    error: null,
    cookingStartTime: null,
    cookingElapsed: 0,
    ...overrides,
  };
}

function agentSession(overrides: Partial<AgentSession>): AgentSession {
  return {
    sessionId: 'session-1',
    clientKind: 'mcp',
    hostLabel: 'Host',
    agentLabel: 'Codex',
    llmModelId: null,
    llmModelLabel: null,
    threadId: 'thread-1',
    messageId: 'msg-1',
    modelId: 'model-1',
    phase: 'rendering',
    statusText: '',
    updatedAt: 1,
    ...overrides,
  };
}

function threadAgentState(overrides: Partial<ThreadAgentState>): ThreadAgentState {
  return {
    agentLabel: 'Codex',
    sessionId: 'session-1',
    llmModelLabel: null,
    providerKind: null,
    connectionState: 'active',
    phase: 'rendering',
    statusText: '',
    busy: true,
    waitingOnPrompt: false,
    updatedAt: 1,
    ...overrides,
  };
}

test('deriveViewerBusyState prefers local request phase over external agent state', () => {
  const result = deriveViewerBusyState({
    activeThreadId: 'thread-1',
    activeVersionId: 'msg-1',
    activeModelId: 'model-1',
    activeThreadRequests: [request({ phase: 'repairing' })],
    activeAgentSessions: [agentSession({ phase: 'rendering', statusText: 'external' })],
    threadAgentState: threadAgentState({ phase: 'rendering', statusText: 'thread' }),
    phase: 'idle',
    isManual: false,
    manualThreadId: null,
    manualMessageId: null,
    repairMessage: 'repair copy',
    cookingPhrase: null,
    hasRenderableModel: true,
    suppressViewportBusyUi: false,
  });

  assert.equal(result.viewerBusyPhase, 'repairing');
  assert.equal(result.viewerBusyText, 'repair copy');
  assert.equal(result.showViewerBusyMask, true);
});

test('deriveViewerBusyState suppresses thread-agent mask when no model is rendered yet', () => {
  const result = deriveViewerBusyState({
    activeThreadId: 'thread-1',
    activeVersionId: 'msg-1',
    activeModelId: 'model-1',
    activeThreadRequests: [],
    activeAgentSessions: [],
    threadAgentState: threadAgentState({ phase: 'rendering' }),
    phase: 'idle',
    isManual: false,
    manualThreadId: null,
    manualMessageId: null,
    repairMessage: null,
    cookingPhrase: null,
    hasRenderableModel: false,
    suppressViewportBusyUi: false,
  });

  assert.equal(result.viewerBusyPhase, 'rendering');
  assert.equal(result.viewerBusyText, 'External agent Codex is updating the model.');
  assert.equal(result.showViewerBusyMask, false);
});

test('deriveViewerBusyState reports manual render when matching active thread/version', () => {
  const result = deriveViewerBusyState({
    activeThreadId: 'thread-1',
    activeVersionId: 'msg-1',
    activeModelId: 'model-1',
    activeThreadRequests: [],
    activeAgentSessions: [],
    threadAgentState: null,
    phase: 'rendering',
    isManual: true,
    manualThreadId: 'thread-1',
    manualMessageId: 'msg-1',
    repairMessage: null,
    cookingPhrase: null,
    hasRenderableModel: true,
    suppressViewportBusyUi: false,
  });

  assert.equal(result.viewerBusyPhase, 'rendering');
  assert.equal(result.viewerBusyText, 'Stabilizing the geometry into manufacturable solids.');
  assert.equal(result.showViewerBusyMask, true);
});
