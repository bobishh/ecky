import assert from 'node:assert/strict';
import test from 'node:test';

import {
  buildOptimisticQueuedDialogueMessage,
  deriveOptimisticDialogueMessages,
  hasLiveApiEngineConnection,
  mergeOptimisticQueuedDialogueMessages,
} from './apiDialogue';
import type { Message, Request } from '../types/domain';

function request(overrides: Partial<Request> = {}): Request {
  return {
    id: 'req-1',
    prompt: 'make me a teapot',
    attachments: [],
    createdAt: 1_710_000_000_000,
    phase: 'classifying',
    attempt: 1,
    maxAttempts: 1,
    maxVerifyAttempts: 0,
    isQuestion: false,
    lightResponse: '',
    screenshot: null,
    threadId: 'thread-1',
    baseMessageId: null,
    baseModelId: null,
    result: null,
    error: null,
    cookingStartTime: null,
    cookingElapsed: 0,
    ...overrides,
  };
}

function message(overrides: Partial<Message> = {}): Message {
  return {
    id: 'msg-1',
    role: 'assistant',
    content: 'hello',
    status: 'success',
    timestamp: 1_710_000_000,
    ...overrides,
  };
}

test('deriveOptimisticDialogueMessages adds immediate user and pending assistant placeholders', () => {
  const messages = deriveOptimisticDialogueMessages([], [request()]);

  assert.equal(messages.length, 2);
  assert.equal(messages[0]?.role, 'user');
  assert.equal(messages[0]?.content, 'make me a teapot');
  assert.equal(messages[0]?.status, 'success');
  assert.equal(messages[1]?.role, 'assistant');
  assert.equal(messages[1]?.status, 'pending');
  assert.match(messages[1]?.content ?? '', /routing|processing/i);
});

test('deriveOptimisticDialogueMessages keeps inline image attachments visible', () => {
  const messages = deriveOptimisticDialogueMessages([], [
    request({
      attachments: [
        {
          path: '',
          name: 'reference.png',
          explanation: '',
          dataUrl: 'data:image/png;base64,reference',
          type: 'image',
        },
      ],
    }),
  ]);

  assert.deepEqual(messages[0]?.attachmentImages, ['data:image/png;base64,reference']);
});

test('deriveOptimisticDialogueMessages drops placeholders once persisted assistant message arrives', () => {
  const persisted = message({
    id: 'msg-persisted',
    role: 'assistant',
    content: 'Cup ready.',
    status: 'pending',
  });
  const messages = deriveOptimisticDialogueMessages(
    [persisted],
    [request({ result: { design: null, threadId: 'thread-1', messageId: 'msg-persisted', stlUrl: '', artifactBundle: null, modelManifest: null } })],
  );

  assert.deepEqual(messages, [persisted]);
});

test('deriveOptimisticDialogueMessages shows terminal error over stale routing copy', () => {
  const messages = deriveOptimisticDialogueMessages([], [
    request({
      phase: 'error',
      lightResponse: 'Routing request...',
      error: 'Structural verification failed: PREVIEW_STL_DISCONNECTED_COMPONENTS',
    }),
  ]);

  assert.equal(messages[1]?.status, 'error');
  assert.equal(messages[1]?.content, 'Structural verification failed: PREVIEW_STL_DISCONNECTED_COMPONENTS');
});

test('mergeOptimisticQueuedDialogueMessages shows pending MCP user message before backend refresh', () => {
  const optimistic = buildOptimisticQueuedDialogueMessage({
    id: 'optimistic-queued-1',
    prompt: 'Message should paint immediately',
    attachments: [],
    timestampMs: 1_710_000_000_000,
  });

  const messages = mergeOptimisticQueuedDialogueMessages(
    [],
    [{ threadId: 'thread-1', message: optimistic }],
    'thread-1',
  );

  assert.equal(messages.length, 1);
  assert.equal(messages[0]?.role, 'user');
  assert.equal(messages[0]?.status, 'pending');
  assert.equal(messages[0]?.content, 'Message should paint immediately');
});

test('mergeOptimisticQueuedDialogueMessages drops MCP optimistic message after persisted id arrives', () => {
  const persisted = message({
    id: 'queued-1',
    role: 'user',
    content: 'Message persisted.',
    status: 'pending',
  });
  const optimistic = buildOptimisticQueuedDialogueMessage({
    id: 'queued-1',
    prompt: 'Message persisted.',
    attachments: [],
  });

  const messages = mergeOptimisticQueuedDialogueMessages(
    [persisted],
    [{ threadId: 'thread-1', message: optimistic }],
    'thread-1',
  );

  assert.deepEqual(messages, [persisted]);
});

test('hasLiveApiEngineConnection requires api mode plus selected engine with key', () => {
  assert.equal(
    hasLiveApiEngineConnection('api_key', {
      provider: 'openai',
      apiKey: 'sk-live',
    }),
    true,
  );
  assert.equal(
    hasLiveApiEngineConnection('api_key', {
      provider: 'openai',
      apiKey: '',
    }),
    false,
  );
  assert.equal(
    hasLiveApiEngineConnection('api_key', {
      provider: 'ollama',
      apiKey: '',
    }),
    true,
  );
  assert.equal(
    hasLiveApiEngineConnection('mcp', {
      provider: 'openai',
      apiKey: 'sk-live',
    }),
    false,
  );
});
