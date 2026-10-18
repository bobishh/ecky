import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

import type { CodexDialogueMessage } from './tauri/contracts';
import {
  applyProviderDialogueAction,
  codexProviderUpdateTarget,
  createProviderDialogueState,
  projectProviderDialogue,
  isProviderResultCurrent,
  type ProviderDialogueSnapshot,
} from './providerDialogueSync';

test('first owned provider update can initialize an unloaded dialogue without a text send', () => {
  const event = { eckyThreadId: 'ecky-1', threadId: 'codex-1', method: 'history/persisted' };
  assert.equal(codexProviderUpdateTarget(event, 'ecky-1', 'provider:codex', null), 'ecky-1');
  assert.equal(codexProviderUpdateTarget(event, 'other', 'provider:codex', null), null);
  assert.equal(codexProviderUpdateTarget(event, 'ecky-1', 'api_key', null), null);
  assert.equal(codexProviderUpdateTarget({ threadId: 'unknown', method: 'history/persisted' }, 'ecky-1', 'provider:codex', null), null);
});

test('provider stream updates need the exact active binding while owned cursor changes can refresh it', () => {
  const binding = { eckyThreadId: 'ecky-1', codexThreadId: 'codex-1' };
  const event = { threadId: 'codex-1', method: 'item/agentMessage/delta' };
  assert.equal(codexProviderUpdateTarget(event, 'ecky-1', 'provider:codex', binding), 'ecky-1');
  assert.equal(codexProviderUpdateTarget({ ...event, threadId: 'codex-old' }, 'ecky-1', 'provider:codex', binding), null);
  assert.equal(codexProviderUpdateTarget({ ...event, eckyThreadId: 'ecky-1', threadId: 'codex-new' }, 'ecky-1', 'provider:codex', binding), 'ecky-1');
});

function message(id: string, timestamp: number, content = id): CodexDialogueMessage {
  return { id, role: 'assistant', content, status: 'success', timestamp, attachments: [] };
}

function snapshot(overrides: Partial<ProviderDialogueSnapshot> = {}): ProviderDialogueSnapshot {
  return {
    providerId: 'codex',
    providerLabel: 'Codex',
    externalConversationId: 'conversation-1',
    messages: [],
    liveMessages: [],
    turnTraces: [],
    nextCursor: 'cursor-1',
    backwardsCursor: null,
    runtime: { phase: 'idle', activeTurnId: null, error: null },
    queue: [],
    ...overrides,
  };
}

test('initial snapshot projects persisted messages with stable order', () => {
  const state = applyProviderDialogueAction(
    createProviderDialogueState(),
    { type: 'snapshot', snapshot: snapshot({ messages: [message('b', 20), message('a', 10)] }), preserveLoadedPages: false },
  );
  assert.deepEqual(state.snapshot?.messages.map((item) => item.id), ['b', 'a']);
  assert.deepEqual(projectProviderDialogue(state.snapshot!).map((item) => item.id), ['b', 'a']);
});

test('Given persisted provider attachment When dialogue projects Then user image remains in history', () => {
  const projected = projectProviderDialogue(snapshot({
    messages: [{
      id: 'user-with-image',
      role: 'user',
      content: 'Use this photo.',
      status: 'success',
      timestamp: 10,
      attachments: [{
        path: '/workspace/photo.png',
        name: 'photo.png',
        explanation: 'Face reference.',
        dataUrl: 'data:image/png;base64,iVBORw0KGgo=',
        kind: 'image',
      }],
    } as any],
  }));

  assert.deepEqual(projected[0]?.attachmentImages, ['data:image/png;base64,iVBORw0KGgo=']);
});

test('older page merge dedupes by identity and keeps page order', () => {
  let state = applyProviderDialogueAction(
    createProviderDialogueState(),
    { type: 'snapshot', snapshot: snapshot({ messages: [message('b', 20), message('c', 30)] }), preserveLoadedPages: false },
  );
  state = applyProviderDialogueAction(state, {
    type: 'page',
    page: { messages: [message('a', 10), message('b', 20, 'newer copy')], nextCursor: null, backwardsCursor: 'back-1' },
    direction: 'older',
  });
  assert.deepEqual(state.snapshot?.messages.map((item) => item.id), ['a', 'b', 'c']);
  assert.equal(state.snapshot?.messages[1]?.content, 'newer copy');
});

test('live messages reconcile into persisted identity without duplicate projection', () => {
  const state = applyProviderDialogueAction(
    applyProviderDialogueAction(createProviderDialogueState(), {
      type: 'snapshot', snapshot: snapshot({ messages: [message('m1', 10, 'persisted')] }), preserveLoadedPages: false,
    }),
    { type: 'live', liveMessages: [{ id: 'm1', role: 'assistant', content: 'live', status: 'working', timestamp: 11, attachments: [] }], runtime: { phase: 'working', activeTurnId: 'turn-1', error: null } },
  );
  const projected = projectProviderDialogue(state.snapshot!);
  assert.equal(projected.filter((item) => item.id === 'm1').length, 1);
  assert.equal(projected[0]?.content, 'live');
});

test('stale load token cannot replace newer state', () => {
  let state = applyProviderDialogueAction(createProviderDialogueState(), { type: 'loadStarted', token: 2 });
  state = applyProviderDialogueAction(state, { type: 'snapshot', snapshot: snapshot({ messages: [message('new', 2)] }), preserveLoadedPages: false, token: 2 });
  const stale = applyProviderDialogueAction(state, { type: 'snapshot', snapshot: snapshot({ messages: [message('old', 1)] }), preserveLoadedPages: false, token: 1 });
  assert.deepEqual(stale.snapshot?.messages.map((item) => item.id), ['new']);
});

test('terminal runtime error remains visible in state', () => {
  const state = applyProviderDialogueAction(
    applyProviderDialogueAction(createProviderDialogueState(), {
      type: 'snapshot', snapshot: snapshot(), preserveLoadedPages: false,
    }),
    { type: 'live', liveMessages: [], runtime: { phase: 'error', activeTurnId: null, error: 'provider exploded' } },
  );
  assert.equal(state.error, 'provider exploded');
  assert.equal(state.snapshot?.runtime.error, 'provider exploded');
});

test('Given initial provider load fails When no snapshot exists Then loading stops and raw error survives', () => {
  const loading = applyProviderDialogueAction(createProviderDialogueState(), {
    type: 'loadStarted',
    token: 1,
  });
  const failed = applyProviderDialogueAction(loading, {
    type: 'error',
    error: 'provider unavailable',
  });

  assert.equal(failed.snapshot, null);
  assert.equal(failed.loading, false);
  assert.equal(failed.error, 'provider unavailable');
});

test('Given a fresh provider snapshot When clearing an old error Then the fresh snapshot stays authoritative', () => {
  let state = applyProviderDialogueAction(createProviderDialogueState(), {
    type: 'snapshot',
    snapshot: snapshot({ messages: [message('fresh', 2)] }),
    preserveLoadedPages: true,
  });
  state = applyProviderDialogueAction(state, {
    type: 'error',
    error: 'old provider failure',
  });
  const cleared = applyProviderDialogueAction(state, { type: 'clearError' });

  assert.equal(cleared.error, null);
  assert.deepEqual(cleared.snapshot?.messages.map((item) => item.id), ['fresh']);
});

test('stale async provider result is rejected after thread switch', () => {
  const current = snapshot({ binding: { eckyThreadId: 'thread-b' } });
  assert.equal(isProviderResultCurrent(current, 'thread-a'), false);
  assert.equal(isProviderResultCurrent(current, 'thread-b'), true);
  assert.equal(isProviderResultCurrent(null, 'thread-b'), false);
});

test('Given an in-flight load When a mutation returns an authoritative snapshot Then the old load cannot erase it', () => {
  let state = applyProviderDialogueAction(createProviderDialogueState(), {
    type: 'loadStarted',
    token: 1,
  });
  state = applyProviderDialogueAction(state, {
    type: 'authoritativeSnapshot',
    snapshot: snapshot({ messages: [message('sent', 3)] }),
    preserveLoadedPages: true,
  });
  const stale = applyProviderDialogueAction(state, {
    type: 'snapshot',
    snapshot: null,
    preserveLoadedPages: false,
    token: 1,
  });

  assert.deepEqual(stale.snapshot?.messages.map((item) => item.id), ['sent']);
  assert.ok(stale.loadToken > 1);
});

test('provider UI projects snapshots while Rust queue supervisors own dispatch admission', () => {
  const app = readFileSync(new URL('../../src/App.svelte', import.meta.url), 'utf8');
  const client = readFileSync(new URL('./tauri/client.ts', import.meta.url), 'utf8');
  const bindings = readFileSync(
    new URL('../../src-tauri/src/bindings.rs', import.meta.url),
    'utf8',
  );

  assert.doesNotMatch(app, /\bdispatchCodexPromptQueue\s*\(/);
  assert.doesNotMatch(app, /\bdispatchAgyPromptQueue\s*\(/);
  assert.doesNotMatch(client, /export async function dispatchCodexPromptQueue/);
  assert.doesNotMatch(client, /export async function dispatchAgyPromptQueue/);
  assert.doesNotMatch(bindings, /dispatch_codex_prompt_queue/);
  assert.doesNotMatch(bindings, /dispatch_agy_prompt_queue/);
});
