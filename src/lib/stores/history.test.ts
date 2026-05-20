import test from 'node:test';
import assert from 'node:assert/strict';

import type { ArtifactBundle, Message, ModelManifest } from '../types/domain';
import {
  activeThreadLoadingId,
  activeThreadMessagesLoading,
  activeThreadVersionLoading,
  createNewThread,
  threadMessagePageState,
} from './history';
import type { Thread } from '../types/domain';
import { activeThreadIdStore, activeVersionId } from './domainState';
import { session } from './sessionStore';
import { get } from 'svelte/store';
import { activeVersionTimelineIndex, versionTimelineMessages } from '../threadTimeline';

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

function mergeThreadMessagesLike(existing: Message[], incoming: Message[]): Message[] {
  const seen = new Set<string>();
  return [...incoming, ...existing].filter((message) => {
    if (seen.has(message.id)) return false;
    seen.add(message.id);
    return true;
  });
}

function mergeThreadMessagePayloadLike(existing: Message | undefined, incoming: Message): Message {
  if (!existing) return incoming;
  return {
    ...existing,
    ...incoming,
    output: incoming.output ?? existing.output,
    artifactBundle: incoming.artifactBundle ?? existing.artifactBundle,
    modelManifest: incoming.modelManifest ?? existing.modelManifest,
  };
}

function versionCountForMessagesLike(messages: Message[], fallback: number): number {
  return Math.max(
    fallback,
    messages.filter((message) => Boolean(message.output || message.artifactBundle || message.modelManifest)).length,
  );
}

function mergeCommittedVersionMessageLike(
  threads: Thread[],
  threadId: string,
  title: string,
  message: Message,
) {
  const existing = threads.find((thread) => thread.id === threadId) ?? null;
  const nextMessages = mergeThreadMessagesLike(existing?.messages ?? [], [message]);
  const nextThread: Thread = existing
    ? {
        ...existing,
        title: title || existing.title,
        messages: nextMessages,
        updatedAt: Math.max(existing.updatedAt ?? 0, message.timestamp),
        versionCount: versionCountForMessagesLike(nextMessages, existing.versionCount ?? 0),
      }
    : {
        id: threadId,
        title,
        summary: '',
        messages: nextMessages,
        updatedAt: message.timestamp,
        versionCount: versionCountForMessagesLike(nextMessages, 0),
        pendingCount: 0,
        queuedCount: 0,
        errorCount: 0,
        status: 'active',
      };

  return [nextThread, ...threads.filter((thread) => thread.id !== threadId)];
}

function mergeActiveThreadMessagesLike(
  existingMessages: Message[],
  incomingMessages: Message[],
  activeMessageId: string | null,
): Message[] {
  const existingById = new Map(existingMessages.map((message) => [message.id, message]));
  const incomingIds = new Set(incomingMessages.map((message) => message.id));
  const mergedIncoming = incomingMessages.map((message) =>
    mergeThreadMessagePayloadLike(existingById.get(message.id), message),
  );

  if (!activeMessageId || incomingIds.has(activeMessageId)) {
    return mergedIncoming;
  }

  const restoredActive = existingById.get(activeMessageId);
  return restoredActive ? [restoredActive, ...mergedIncoming] : mergedIncoming;
}

function beginThreadSwitchLike(targetThreadId: string) {
  activeVersionId.set(null);
  session.setError(null);
  session.setStlUrl(null);
  session.clearModelRuntime();
  activeThreadIdStore.set(targetThreadId);
}

function detachActiveVersionRuntimeLike() {
  activeVersionId.set(null);
  session.setStlUrl(null);
  session.clearModelRuntime();
}

function effectiveActiveVersionIdLike(messages: Message[], currentVersionId: string | null): string | null {
  const versions = versionTimelineMessages(messages);
  const index = activeVersionTimelineIndex(versions, currentVersionId);
  return index >= 0 ? versions[index]?.id ?? null : null;
}

const versionRuntimePayloadCacheLike = new Map<
  string,
  { artifactBundle: Message['artifactBundle'] | null; modelManifest: Message['modelManifest'] | null }
>();

function resetVersionRuntimePayloadCacheLike() {
  versionRuntimePayloadCacheLike.clear();
}

function rememberVersionRuntimePayloadLike(
  messageId: string,
  artifactBundle: Message['artifactBundle'] | null | undefined,
  modelManifest: Message['modelManifest'] | null | undefined,
) {
  if (!artifactBundle || !modelManifest || artifactBundle.modelId !== modelManifest.modelId) return;
  versionRuntimePayloadCacheLike.set(messageId, {
    artifactBundle,
    modelManifest,
  });
}

function resolveVersionRuntimePayloadLike(message: Message) {
  const cached = versionRuntimePayloadCacheLike.get(message.id);
  if (cached && cached.artifactBundle && cached.modelManifest && cached.artifactBundle.modelId === cached.modelManifest.modelId) {
    return cached;
  }
  return {
    artifactBundle: message.artifactBundle ?? null,
    modelManifest: message.modelManifest ?? null,
  };
}

async function persistVersionRuntimePayloadLike(
  messageId: string,
  artifactBundle: Message['artifactBundle'] | null | undefined,
  modelManifest: Message['modelManifest'] | null | undefined,
  persistRuntime?: (
    messageId: string,
    artifactBundle: Message['artifactBundle'],
    modelManifest: Message['modelManifest'],
  ) => Promise<void>,
) {
  if (!artifactBundle || !modelManifest || artifactBundle.modelId !== modelManifest.modelId) {
    return false;
  }
  if (persistRuntime) {
    await persistRuntime(messageId, artifactBundle, modelManifest);
  }
  return true;
}

test('resolveVersionRuntimePayload prefers remembered rebuilt runtime for same message', () => {
  resetVersionRuntimePayloadCacheLike();

  const staleBundle = sampleBundle('model-1', '/tmp/stale-preview.stl');
  const rebuiltBundle = sampleBundle('model-1', '/tmp/rebuilt-preview.stl');
  const manifest = sampleManifest('model-1');
  const message = sampleMessage('msg-1', staleBundle, manifest);

  rememberVersionRuntimePayloadLike(message.id, rebuiltBundle, manifest);
  const resolved = resolveVersionRuntimePayloadLike(message);

  assert.equal(resolved.artifactBundle?.previewStlPath, rebuiltBundle.previewStlPath);
  assert.equal(resolved.modelManifest?.modelId, manifest.modelId);
});

test('resolveVersionRuntimePayload uses target artifact instead of previous current session when switching versions', () => {
  resetVersionRuntimePayloadCacheLike();

  const currentBundle = sampleBundle('model-current', '/tmp/current-preview.stl');
  const currentManifest = sampleManifest('model-current');
  const targetBundle = sampleBundle('model-target', '/tmp/target-preview.stl');
  const targetManifest = sampleManifest('model-target');
  const targetMessage = sampleMessage('msg-target', targetBundle, targetManifest);

  activeThreadIdStore.set('thread-1');
  activeVersionId.set(targetMessage.id);
  session.setStlUrl('/tmp/current-preview.stl');
  session.setModelRuntime(currentBundle, currentManifest);

  const resolved = resolveVersionRuntimePayloadLike(targetMessage);

  assert.equal(resolved.artifactBundle?.modelId, 'model-target');
  assert.equal(resolved.artifactBundle?.previewStlPath, '/tmp/target-preview.stl');
  assert.equal(resolved.modelManifest?.modelId, 'model-target');
});

test('persistVersionRuntimePayload skips inconsistent runtime payloads', async () => {
  const calls: Array<{ messageId: string; modelId: string }> = [];
  const persisted = await persistVersionRuntimePayloadLike(
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
  const persisted = await persistVersionRuntimePayloadLike(
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

  const merged = mergeCommittedVersionMessageLike(
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

  const merged = mergeActiveThreadMessagesLike([active], [older], 'msg-active');

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

  const merged = mergeActiveThreadMessagesLike([active], [skinny], 'msg-active');

  assert.equal(merged[0]?.id, 'msg-active');
  assert.equal(merged[0]?.artifactBundle?.previewStlPath, '/tmp/active.stl');
  assert.equal(merged[0]?.modelManifest?.modelId, 'model-active');
});

test('effectiveActiveVersionId falls back to displayed latest version when active id is a draft preview', () => {
  const older = sampleMessage(
    'msg-older',
    sampleBundle('model-older', '/tmp/older.stl'),
    sampleManifest('model-older'),
  );
  const latest = {
    ...sampleMessage(
      'msg-latest',
      sampleBundle('model-latest', '/tmp/latest.stl'),
      sampleManifest('model-latest'),
    ),
    timestamp: older.timestamp + 1,
  };

  const effective = effectiveActiveVersionIdLike([older, latest], 'draft-preview-id');

  assert.equal(effective, 'msg-latest');
});

test('Given thread switch starts When previous model is still loaded Then stale version runtime is detached before new thread becomes active', () => {
  const oldBundle = sampleBundle('model-old', '/tmp/old.stl');
  const oldManifest = sampleManifest('model-old');

  activeThreadIdStore.set('thread-old');
  activeVersionId.set('message-old');
  session.setStlUrl('/tmp/old.stl');
  session.setModelRuntime(oldBundle, oldManifest);

  beginThreadSwitchLike('thread-new');

  assert.equal(get(activeThreadIdStore), 'thread-new');
  assert.equal(get(activeVersionId), null);
  assert.equal(get(session).stlUrl, null);
  assert.equal(get(session).artifactBundle, null);
  assert.equal(get(session).modelManifest, null);
});

test('Given active version is removed When fallback version is still resolving Then stale viewport runtime is detached', () => {
  const oldBundle = sampleBundle('model-old', '/tmp/old.stl');
  const oldManifest = sampleManifest('model-old');

  activeThreadIdStore.set('thread-1');
  activeVersionId.set('message-old');
  session.setStlUrl('/tmp/old.stl');
  session.setModelRuntime(oldBundle, oldManifest);

  detachActiveVersionRuntimeLike();

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
