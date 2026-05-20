import assert from 'node:assert/strict';
import test from 'node:test';

import type { ArtifactBundle, Message, ModelManifest } from '../types/domain';

function sampleBundle(modelId: string): ArtifactBundle {
  return {
    schemaVersion: 1,
    modelId,
    sourceKind: 'generated',
    engineKind: 'freecad',
    sourceLanguage: 'legacyPython',
    geometryBackend: 'freecad',
    contentHash: `hash-${modelId}`,
    artifactVersion: 1,
    fcstdPath: `/tmp/${modelId}.FCStd`,
    manifestPath: `/tmp/${modelId}.json`,
    macroPath: `/tmp/${modelId}.FCMacro`,
    previewStlPath: `/tmp/${modelId}.stl`,
    viewerAssets: [],
  };
}

function sampleManifest(modelId: string): ModelManifest {
  return {
    modelId,
    sourceKind: 'generated',
    sourceLanguage: 'legacyPython',
    geometryBackend: 'freecad',
    document: {
      documentName: modelId,
      documentLabel: modelId,
      objectCount: 1,
      warnings: [],
    },
    parts: [],
    parameterGroups: [],
    controlPrimitives: [],
    controlRelations: [],
    controlViews: [],
    selectionTargets: [],
    advisories: [],
    measurementAnnotations: [],
    warnings: [],
    enrichmentState: { status: 'none', proposals: [] },
  };
}

function message(id: string, patch: Partial<Message> = {}): Message {
  return {
    id,
    role: 'assistant',
    content: 'Cached model',
    status: 'success',
    output: {
      title: 'Cached model',
      versionName: 'Cached',
      response: '',
      interactionMode: 'design',
      macroCode: '# cached macro',
      sourceLanguage: 'legacyPython',
      geometryBackend: 'freecad',
      uiSpec: { fields: [] },
      initialParams: {},
      postProcessing: null,
    } as Message['output'],
    usage: null,
    artifactBundle: sampleBundle('cached-model'),
    modelManifest: sampleManifest('cached-model'),
    agentOrigin: null,
    imageData: null,
    visualKind: null,
    attachmentImages: [],
    timestamp: 100,
    ...patch,
  };
}

function mergeRestoredThreadMessagesLike(
  existingMessages: Message[],
  incomingMessages: Message[],
  activeMessageId: string | null,
): Message[] {
  const existingById = new Map(existingMessages.map((message) => [message.id, message]));
  const incomingIds = new Set(incomingMessages.map((message) => message.id));
  const mergedIncoming = incomingMessages.map((message) => {
    const existing = existingById.get(message.id);
    if (!existing) return message;
    return {
      ...existing,
      ...message,
      output: message.output ?? existing.output,
      artifactBundle: message.artifactBundle ?? existing.artifactBundle,
      modelManifest: message.modelManifest ?? existing.modelManifest,
    };
  });

  if (!activeMessageId || incomingIds.has(activeMessageId)) {
    return mergedIncoming;
  }

  const restoredActive = existingById.get(activeMessageId);
  return restoredActive ? [restoredActive, ...mergedIncoming] : mergedIncoming;
}

test('mergeRestoredThreadMessages preserves restored active runtime when page returns skinny same message', () => {
  const restored = message('msg-cached');
  const skinny = message('msg-cached', {
    content: 'Skinny page copy',
    output: null,
    artifactBundle: null,
    modelManifest: null,
  });

  const merged = mergeRestoredThreadMessagesLike([restored], [skinny], 'msg-cached');

  assert.equal(merged.length, 1);
  assert.equal(merged[0].content, 'Skinny page copy');
  assert.equal(merged[0].output?.macroCode, '# cached macro');
  assert.equal(merged[0].artifactBundle?.previewStlPath, '/tmp/cached-model.stl');
  assert.equal(merged[0].modelManifest?.modelId, 'cached-model');
});

test('mergeRestoredThreadMessages keeps restored active version when first page omits it', () => {
  const restored = message('msg-cached');
  const older = message('msg-older', {
    content: 'Older model',
    artifactBundle: sampleBundle('older-model'),
    modelManifest: sampleManifest('older-model'),
    timestamp: 90,
  });

  const merged = mergeRestoredThreadMessagesLike([restored], [older], 'msg-cached');

  assert.equal(merged[0].id, 'msg-cached');
  assert.equal(merged[0].artifactBundle?.modelId, 'cached-model');
  assert.equal(merged[1].id, 'msg-older');
});
