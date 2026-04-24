import assert from 'node:assert/strict';
import test from 'node:test';

import { deriveAgentOpsState } from './agentOps';
import type { AgentSession, Request } from '../types/domain';
import type { ThreadAgentState } from '../tauri/client';
import type { AgentTerminalSnapshot } from '../types/domain';

function request(threadId: string, requestId: string): Request {
  return {
    id: requestId,
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
    threadId,
    baseMessageId: null,
    baseModelId: null,
    result: null,
    error: null,
    cookingStartTime: null,
    cookingElapsed: 0,
  };
}

function threadAgentState(): ThreadAgentState {
  return {
    agentLabel: 'Codex',
    sessionId: 'session-1',
    llmModelLabel: 'Model',
    providerKind: 'mcp',
    connectionState: 'active',
    phase: 'rendering',
    statusText: '',
    busy: true,
    waitingOnPrompt: false,
    updatedAt: 1,
    activityStartedAt: 100,
    activityLabel: '',
  };
}

function terminal(): AgentTerminalSnapshot {
  return {
    agentId: 'agent-1',
    agentLabel: 'Codex',
    sessionId: 'session-1',
    active: true,
    summary: 'drawing lines',
    updatedAt: 1,
  } as AgentTerminalSnapshot;
}

function session(): AgentSession {
  return {
    sessionId: 'session-1',
    clientKind: 'mcp',
    hostLabel: 'Host',
    agentLabel: 'Codex',
    llmModelId: null,
    llmModelLabel: 'Model',
    threadId: 'thread-1',
    messageId: 'msg-1',
    modelId: 'model-1',
    phase: 'rendering',
    statusText: '',
    updatedAt: 1,
  };
}

test('deriveAgentOpsState keeps attention off the current thread and resolves agent bubble text', () => {
  const state = deriveAgentOpsState({
    activeThreadId: 'thread-1',
    activeVersionId: 'msg-1',
    activeThreadRequests: [request('thread-1', 'req-1')],
    activeAgentSessions: [session()],
    threadAgentState: threadAgentState(),
    visibleAgentTerminal: terminal(),
    pendingAgentPrompts: [
      {
        requestId: 'prompt-1',
        agentLabel: 'Codex',
        threadId: 'thread-1',
      },
      {
        requestId: 'prompt-2',
        agentLabel: 'Codex',
        threadId: 'thread-2',
      },
    ],
    pendingViewportScreenshotChoices: [
      {
        requestId: 'shot-1',
        threadId: 'thread-2',
        messageId: 'msg-2',
        modelId: null,
        previewStlPath: '/tmp/model.stl',
        viewerAssets: [],
        includeOverlays: false,
        message: 'Choose',
        buttons: ['Current View'],
        camera: null,
      },
    ],
    connectionType: 'mcp',
    mcpMode: 'active',
    autoAgents: [{ id: 'codex', label: 'Codex', cmd: 'codex', args: [], enabled: true }],
    primaryAgentId: 'codex',
    primaryAgentLabel: 'Codex',
    cookingPhrase: 'Preparing update.',
    nowSecs: 160,
    hasRenderableModel: true,
    suppressViewportBusyUi: false,
  });

  assert.equal(state.activePendingAgentPrompt?.requestId, 'prompt-1');
  assert.deepEqual(state.threadAttentionIds, ['thread-2']);
  assert.equal(state.activeViewportScreenshotChoice, null);
  assert.equal(state.activeMcpBusy, true);
  assert.equal(state.activeMcpRenderBusy, true);
  assert.equal(state.activeMcpBubbleSummary, 'Preparing update. · 1m 00s');
  assert.equal(state.activeAgentTerminalMetaSummary, 'drawing lines · 1m 00s');
  assert.equal(state.activeMascotAgentIdentity, 'Codex');
  assert.equal(state.hasLiveMcpSession, true);
});
