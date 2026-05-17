import test from 'node:test';
import assert from 'node:assert/strict';

import type { ArtifactBundle, Message, ModelManifest } from '../types/domain';
import {
  activeThreadLoadingId,
  activeThreadMessagesLoading,
  activeThreadVersionLoading,
  beginThreadSwitchForTests,
  createNewThread,
  mergeActiveThreadMessagesForTests,
  rememberVersionRuntimePayloadForTests,
  mergeCommittedVersionMessageForTests,
  persistVersionRuntimePayloadForTests,
  resolveVersionRuntimePayloadForTests,
  resetVersionRuntimePayloadCacheForTests,
  threadMessagePageState,
} from './history';
import type { Thread } from '../types/domain';
import { activeThreadIdStore, activeVersionId } from './domainState';
import { session } from './sessionStore';
import { get } from 'svelte/store';

function sampleBundle(modelId: string, previewStlPath: string): ArtifactBundle {
  return {
    schemaVersion: 1,
    modelId,
    sourceKind: 'generated',
    engineKind: 'ecky',
    sourceLanguage: 'ecky',
    geometryBackend: 'build123d',
    contentHash: `hash-${modelId}`,
    artifactVersion: 1,
    fcstdPath: '',
    manifestPath: `/tmp/${modelId}.json`,
    macroPath: `/tmp/${modelId}.ecky`,
    previewStlPath,
    viewerAssets: [],
    edgeTargets: [],
    calloutAnchors: [],
    measurementGuides: [],
    exportArtifacts: [],
  };
}

function sampleManifest(modelId: string): ModelManifest {
  return {
    modelId,
    sourceKind: 'generated',
    sourceLanguage: 'ecky',
    geometryBackend: 'build123d',
    document: {
      documentName: 'Test',
      documentLabel: 'Test',
      objectCount: 1,
      warnings: [],
    },
    parts: [],
    parameterGroups: [],
    selectionTargets: [],
    warnings: [],
    controlPrimitives: [],
    controlViews: [],
    controlRelations: [],
    enrichmentState: { status: 'none', proposals: [] },
  };
}

function sampleMessage(
  id: string,
  artifactBundle: ArtifactBundle,
  modelManifest: ModelManifest,
): Message {
  return {
    id,
    role: 'assistant',
    content: 'Version',
    status: 'success',
    output: null,
    usage: null,
    artifactBundle,
    modelManifest,
    agentOrigin: null,
    imageData: null,
    visualKind: null,
    attachmentImages: [],
    timestamp: Date.now(),
  };
}

function sampleThread(id: string, messages: Message[] = []): Thread {
  return {
    id,
    title: id,
    summary: '',
    messages,
    updatedAt: 1,
    versionCount: messages.length,
    pendingCount: 0,
    queuedCount: 0,
    errorCount: 0,
    status: 'active',
  };
}

test('resolveVersionRuntimePayload prefers remembered rebuilt runtime for same message', () => {
  resetVersionRuntimePayloadCacheForTests();

  const staleBundle = sampleBundle('model-1', '/tmp/stale-preview.stl');
  const rebuiltBundle = sampleBundle('model-1', '/tmp/rebuilt-preview.stl');
  const manifest = sampleManifest('model-1');
  const message = sampleMessage('msg-1', staleBundle, manifest);

  rememberVersionRuntimePayloadForTests(message.id, rebuiltBundle, manifest);
  const resolved = resolveVersionRuntimePayloadForTests(message);

  assert.equal(resolved.artifactBundle?.previewStlPath, rebuiltBundle.previewStlPath);
  assert.equal(resolved.modelManifest?.modelId, manifest.modelId);
});

test('persistVersionRuntimePayload skips inconsistent runtime payloads', async () => {
  const calls: Array<{ messageId: string; modelId: string }> = [];
  const persisted = await persistVersionRuntimePayloadForTests(
    'msg-1',
    sampleBundle('model-1', '/tmp/rebuilt-preview.stl'),
    sampleManifest('model-2'),
    async (messageId, artifactBundle) => {
      calls.push({ messageId, modelId: artifactBundle.modelId });
    },
  );

  assert.equal(persisted, false);
  assert.deepEqual(calls, []);
});

test('persistVersionRuntimePayload stores rebuilt runtime for same message', async () => {
  const calls: Array<{ messageId: string; modelId: string }> = [];
  const persisted = await persistVersionRuntimePayloadForTests(
    'msg-1',
    sampleBundle('model-1', '/tmp/rebuilt-preview.stl'),
    sampleManifest('model-1'),
    async (messageId, artifactBundle) => {
      calls.push({ messageId, modelId: artifactBundle.modelId });
    },
  );

  assert.equal(persisted, true);
  assert.deepEqual(calls, [{ messageId: 'msg-1', modelId: 'model-1' }]);
});

test('mergeCommittedVersionMessage inserts committed fork message into new active thread', () => {
  const bundle = sampleBundle('model-1', '/tmp/preview.stl');
  const manifest = sampleManifest('model-1');
  const message = sampleMessage('msg-fork', bundle, manifest);

  const merged = mergeCommittedVersionMessageForTests(
    [sampleThread('thread-old')],
    'thread-fork',
    'Forked Box',
    message,
  );

  assert.equal(merged[0].id, 'thread-fork');
  assert.equal(merged[0].title, 'Forked Box');
  assert.equal(merged[0].messages[0]?.id, 'msg-fork');
  assert.equal(merged[0].versionCount, 1);
  assert.equal(merged[1].id, 'thread-old');
});

test('mergeActiveThreadMessages preserves seeded active version when first page omits it', () => {
  const active = sampleMessage('msg-active', sampleBundle('model-active', '/tmp/active.stl'), sampleManifest('model-active'));
  const older = sampleMessage('msg-older', sampleBundle('model-older', '/tmp/older.stl'), sampleManifest('model-older'));

  const merged = mergeActiveThreadMessagesForTests([active], [older], 'msg-active');

  assert.deepEqual(merged.map((message) => message.id), ['msg-active', 'msg-older']);
});

test('mergeActiveThreadMessages hydrates skinny page payload from seeded active version', () => {
  const active = sampleMessage('msg-active', sampleBundle('model-active', '/tmp/active.stl'), sampleManifest('model-active'));
  const skinny: Message = {
    ...active,
    output: null,
    artifactBundle: null,
    modelManifest: null,
  };

  const merged = mergeActiveThreadMessagesForTests([active], [skinny], 'msg-active');

  assert.equal(merged[0]?.id, 'msg-active');
  assert.equal(merged[0]?.artifactBundle?.previewStlPath, '/tmp/active.stl');
  assert.equal(merged[0]?.modelManifest?.modelId, 'model-active');
});

test('Given thread switch starts When previous model is still loaded Then stale version runtime is detached before new thread becomes active', () => {
  const oldBundle = sampleBundle('model-old', '/tmp/old.stl');
  const oldManifest = sampleManifest('model-old');

  activeThreadIdStore.set('thread-old');
  activeVersionId.set('message-old');
  session.setStlUrl('/tmp/old.stl');
  session.setModelRuntime(oldBundle, oldManifest);

  beginThreadSwitchForTests('thread-new');

  assert.equal(get(activeThreadIdStore), 'thread-new');
  assert.equal(get(activeVersionId), null);
  assert.equal(get(session).stlUrl, null);
  assert.equal(get(session).artifactBundle, null);
  assert.equal(get(session).modelManifest, null);
});

test('Given stale thread messages are loading When blank thread starts Then loading state is cleared', () => {
  activeThreadIdStore.set('thread-old');
  activeVersionId.set('message-old');
  activeThreadLoadingId.set('thread-old');
  activeThreadMessagesLoading.set(true);
  activeThreadVersionLoading.set(true);
  threadMessagePageState.set({
    'thread-old': {
      isLoading: true,
      hasMore: false,
      nextBefore: null,
      error: null,
    },
  });

  createNewThread({ mode: 'blank' });

  const newThreadId = get(activeThreadIdStore);
  assert.ok(newThreadId);
  assert.notEqual(newThreadId, 'thread-old');
  assert.equal(get(activeVersionId), null);
  assert.equal(get(activeThreadLoadingId), null);
  assert.equal(get(activeThreadMessagesLoading), false);
  assert.equal(get(activeThreadVersionLoading), false);
  assert.equal(get(threadMessagePageState)[newThreadId!]?.isLoading, false);
});
