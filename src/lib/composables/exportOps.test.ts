import assert from 'node:assert/strict';
import test from 'node:test';

import { deriveExportState } from './exportOps';
import type { ArtifactBundle, Message } from '../types/domain';

function bundle(viewerAssetCount: number): ArtifactBundle {
  return {
    schemaVersion: 2,
    modelId: 'model-1',
    sourceKind: 'generated',
    contentHash: 'hash-1',
    artifactVersion: 4,
    fcstdPath: '/tmp/model.FCStd',
    manifestPath: '/tmp/model.json',
    macroPath: '/tmp/model.py',
    previewStlPath: '/tmp/model.stl',
    viewerAssets: Array.from({ length: viewerAssetCount }, (_, index) => ({
      partId: `part-${index}`,
      nodeId: `node-${index}`,
      objectName: `Object${index}`,
      label: index === 0 ? 'Body' : 'Cap',
      path: `/tmp/part-${index}.stl`,
      format: 'stl',
    })),
    exportArtifacts: [
      { label: 'STEP', format: 'step', path: '/tmp/model.step', role: 'cad-exchange' },
    ],
    edgeTargets: [],
    calloutAnchors: [],
    measurementGuides: [],
  };
}

function versionMessage(): Message {
  return {
    id: 'msg-1',
    role: 'assistant',
    content: 'done',
    status: 'success',
    output: {
      title: 'Lamp Shade / Final',
      versionName: 'V1',
      response: 'done',
      interactionMode: 'question',
      macroCode: 'print("ok")',
      sourceLanguage: 'legacyPython',
      geometryBackend: 'mesh',
      uiSpec: { fields: [] },
      initialParams: {},
    },
    usage: null,
    artifactBundle: null,
    modelManifest: null,
    agentOrigin: null,
    imageData: null,
    visualKind: null,
    attachmentImages: [],
    timestamp: 1,
    deletedAt: null,
  };
}

test('deriveExportState resolves filenames and multipart export options from the bundle', () => {
  const state = deriveExportState({
    activeArtifactBundle: bundle(2),
    activeVersionMessage: versionMessage(),
    activeThreadTitle: 'Thread Title',
  });

  assert.equal(state.exportModelTitle, 'Lamp Shade / Final');
  assert.deepEqual(state.exportDefaultNames, {
    threeMf: 'lamp-shade-final.3mf',
    multipartStlZip: 'lamp-shade-final-parts.zip',
    stl: 'lamp-shade-final.stl',
    fcstd: 'lamp-shade-final.FCStd',
    step: 'lamp-shade-final.step',
  });
  assert.deepEqual(
    state.exportOptions.map((option) => option.id),
    ['3mf', 'multipartStlZip', 'stl', 'fcstd', 'step'],
  );
  assert.equal(state.exportOptions.find((option) => option.id === 'step')?.disabled, false);
  assert.equal(state.hasMultipartExportModel, true);
  assert.equal(state.multipartExportParts.length, 2);
  assert.equal(state.canExportModel, true);
});
