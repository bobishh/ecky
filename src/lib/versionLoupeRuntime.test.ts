import assert from 'node:assert/strict';
import test from 'node:test';

import { resolveVersionLoupeRuntime } from './versionLoupeRuntime';
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
    previewStlPath: '/tmp/preview.stl',
    viewerAssets: [
      {
        partId: 'body',
        nodeId: 'body-node',
        objectName: 'Body001',
        label: 'Body',
        path: '/tmp/body.stl',
        format: 'stl',
      },
    ],
    edgeTargets: [],
    calloutAnchors: [],
    measurementGuides: [],
  };
}

function message(): Pick<Message, 'id' | 'artifactBundle' | 'modelManifest' | 'output'> {
  return {
    id: 'version-1',
    artifactBundle: bundle(),
    modelManifest: {
      modelId: 'model-1',
      sourceKind: 'generated',
      sourceLanguage: 'legacyPython',
      geometryBackend: 'freecad',
      document: {
        documentName: 'Test',
        documentLabel: 'Test',
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
    },
    output: {
      title: 'Test',
      versionName: 'V1',
      response: '',
      interactionMode: 'design',
      macroCode: 'cube()',
      sourceLanguage: 'legacyPython',
      geometryBackend: 'freecad',
      uiSpec: { fields: [] },
      initialParams: {},
      postProcessing: null,
    },
  };
}

test('resolveVersionLoupeRuntime returns renderable preview urls when runtime exists', async () => {
  const resolved = await resolveVersionLoupeRuntime(
    message(),
    'thread-1',
    (path) => `asset:${path ?? ''}`,
    {
      inspectRuntime: async (bundle) => ({
        bundle: bundle ?? null,
        previewAvailable: true,
        degradedToPreview: false,
        skippedOversizedPreview: false,
      }),
    },
  );

  assert.equal(resolved.available, true);
  assert.equal(resolved.previewUrl, 'asset:/tmp/preview.stl');
  assert.equal(resolved.viewerAssets[0]?.path, 'asset:/tmp/body.stl');
});

test('resolveVersionLoupeRuntime hides the viewer when runtime is gone', async () => {
  const resolved = await resolveVersionLoupeRuntime(
    {
      ...message(),
      output: null,
    },
    'thread-1',
    (path) => `asset:${path ?? ''}`,
    {
      getThreadMessageVersion: async () => null,
      inspectRuntime: async () => ({
        bundle: null,
        previewAvailable: false,
        degradedToPreview: false,
        skippedOversizedPreview: false,
      }),
    },
  );

  assert.equal(resolved.available, false);
  assert.equal(resolved.previewUrl, null);
  assert.deepEqual(resolved.viewerAssets, []);
});

test('resolveVersionLoupeRuntime rebuilds missing runtime from source payload and persists it', async () => {
  const calls: string[] = [];
  const resolved = await resolveVersionLoupeRuntime(
    message(),
    'thread-1',
    (path) => `asset:${path ?? ''}`,
    {
      inspectRuntime: async (bundle) => {
        calls.push(`inspect:${bundle?.modelId ?? 'null'}`);
        if (bundle?.modelId === 'model-rebuilt') {
          return {
            bundle,
            previewAvailable: true,
            degradedToPreview: false,
            skippedOversizedPreview: false,
          };
        }
        return {
          bundle: null,
          previewAvailable: false,
          degradedToPreview: false,
          skippedOversizedPreview: false,
        };
      },
      renderModel: async () => ({
        ...bundle(),
        modelId: 'model-rebuilt',
        contentHash: 'hash-rebuilt',
        previewStlPath: '/tmp/rebuilt-preview.stl',
      }),
      getModelManifest: async (modelId) => ({
        ...message().modelManifest!,
        modelId,
      }),
      updateVersionRuntime: async (messageId, artifactBundle, modelManifest) => {
        calls.push(`persist:${messageId}:${artifactBundle.modelId}:${modelManifest.modelId}`);
      },
    },
  );

  assert.equal(resolved.available, true);
  assert.equal(resolved.previewUrl, 'asset:/tmp/rebuilt-preview.stl');
  assert.deepEqual(calls, [
    'inspect:model-1',
    'inspect:model-rebuilt',
    'persist:version-1:model-rebuilt:model-rebuilt',
  ]);
});
