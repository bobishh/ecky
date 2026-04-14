import assert from 'node:assert/strict';
import test from 'node:test';
import { modelEngineLabel } from '../modelEngineLabel';
import type { Message } from '../types/domain';

function message(overrides: Partial<Message>): Message {
  return {
    id: 'msg',
    role: 'assistant',
    content: '',
    status: 'success',
    output: null,
    usage: null,
    artifactBundle: null,
    modelManifest: null,
    agentOrigin: null,
    imageData: null,
    visualKind: null,
    attachmentImages: [],
    timestamp: 1,
    ...overrides,
  };
}

test('modelEngineLabel prefers artifact metadata over stale output defaults', () => {
  assert.equal(
    modelEngineLabel(
      message({
        output: {
          title: 'model',
          versionName: 'v1',
          response: '',
          interactionMode: 'design',
          macroCode: '',
          macroDialect: 'legacy',
          sourceLanguage: 'legacyPython',
          geometryBackend: 'freecad',
          engineKind: 'freecad',
          uiSpec: { fields: [] },
          initialParams: {},
          postProcessing: null,
        },
        artifactBundle: {
          modelId: 'model',
          sourceKind: 'generated',
          sourceLanguage: 'build123d',
          geometryBackend: 'build123d',
          engineKind: 'build123d',
          contentHash: 'hash',
          fcstdPath: '',
          manifestPath: '',
          previewStlPath: '',
          viewerAssets: [],
          edgeTargets: [],
          exportArtifacts: [],
        },
      }),
    ),
    'Python + build123d',
  );
});

test('modelEngineLabel distinguishes IR lowered into build123d', () => {
  assert.equal(
    modelEngineLabel(
      message({
        modelManifest: {
          modelId: 'model',
          sourceKind: 'generated',
          sourceLanguage: 'eckyIrV0',
          geometryBackend: 'build123d',
          engineKind: 'eckyIrV0',
          document: { documentName: 'model', documentLabel: 'model' },
          parts: [],
          parameterGroups: [],
          controlPrimitives: [],
          controlRelations: [],
          controlViews: [],
          advisories: [],
          selectionTargets: [],
          measurementAnnotations: [],
          warnings: [],
          enrichmentState: { status: 'none', proposals: [] },
        },
      }),
    ),
    'IR + build123d',
  );
});
