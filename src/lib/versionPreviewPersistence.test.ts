import assert from 'node:assert/strict';
import test from 'node:test';

import { shouldPersistVersionPreview } from './versionPreviewPersistence';
import type { ArtifactBundle, Message } from './types/domain';

function bundle(): ArtifactBundle {
  return {
    schemaVersion: 2,
    modelId: 'model-1',
    sourceKind: 'generated',
    contentHash: 'hash-1',
    artifactVersion: 1,
    fcstdPath: '/tmp/model.FCStd',
    manifestPath: '/tmp/model.json',
    macroPath: '/tmp/model.ecky',
    previewStlPath: '/tmp/model.stl',
    viewerAssets: [],
    edgeTargets: [],
    calloutAnchors: [],
    measurementGuides: [],
  };
}

function versionMessage(imageData: string | null): Message {
  return {
    id: 'version-1',
    role: 'assistant',
    content: 'ready',
    status: 'success',
    output: {
      title: 'Bracket',
      versionName: 'V-1',
      response: 'ready',
      interactionMode: 'design',
      macroCode: '(model)',
      macroDialect: 'ecky',
      sourceLanguage: 'ecky',
      geometryBackend: 'build123d',
      engineKind: 'ecky',
      uiSpec: { fields: [] },
      initialParams: {},
      postProcessing: null,
    },
    usage: null,
    artifactBundle: bundle(),
    modelManifest: null,
    agentOrigin: null,
    imageData,
    visualKind: null,
    attachmentImages: [],
    timestamp: 1,
    deletedAt: null,
  };
}

test('shouldPersistVersionPreview skips existing previews for the matching artifact', () => {
  assert.equal(
    shouldPersistVersionPreview(
      versionMessage('data:image/png;base64,abc'),
      bundle(),
      'asset:/tmp/model.stl',
    ),
    false,
  );
});

test('shouldPersistVersionPreview backfills missing preview for the matching artifact', () => {
  assert.equal(
    shouldPersistVersionPreview(versionMessage(null), bundle(), 'asset:/tmp/model.stl'),
    true,
  );
});

test('shouldPersistVersionPreview tolerates rebuilt preview paths for the same artifact', () => {
  assert.equal(
    shouldPersistVersionPreview(
      versionMessage(null),
      { ...bundle(), previewStlPath: '/tmp/rebuilt/model.stl' },
      'asset:/tmp/rebuilt/model.stl',
    ),
    true,
  );
});

test('shouldPersistVersionPreview rejects screenshots from a different runtime artifact', () => {
  assert.equal(
    shouldPersistVersionPreview(
      versionMessage(null),
      { ...bundle(), modelId: 'other-model', previewStlPath: '/tmp/other.stl' },
      'asset:/tmp/other.stl',
    ),
    false,
  );
});
