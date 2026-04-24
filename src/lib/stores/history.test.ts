import test from 'node:test';
import assert from 'node:assert/strict';

import type { ArtifactBundle, Message, ModelManifest } from '../types/domain';
import {
  rememberVersionRuntimePayloadForTests,
  persistVersionRuntimePayloadForTests,
  resolveVersionRuntimePayloadForTests,
  resetVersionRuntimePayloadCacheForTests,
} from './history';

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
