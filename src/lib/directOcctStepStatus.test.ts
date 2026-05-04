import assert from 'node:assert/strict';
import test from 'node:test';

import { buildDirectOcctStepStatus } from './directOcctStepStatus';
import type { ArtifactBundle, RuntimeCapabilities } from './types/domain';

function meshEckyBundle(): ArtifactBundle {
  return {
    schemaVersion: 1,
    modelId: 'model-1',
    sourceKind: 'generated',
    engineKind: 'ecky',
    sourceLanguage: 'ecky',
    geometryBackend: 'mesh',
    contentHash: 'hash',
    artifactVersion: 1,
    fcstdPath: '/tmp/model.FCStd',
    manifestPath: '/tmp/model.json',
    previewStlPath: '/tmp/model.stl',
    viewerAssets: [],
    exportArtifacts: [],
  };
}

function capabilities(available: boolean, detail: string): RuntimeCapabilities {
  return {
    freecad: { available: true, detail: 'FreeCAD ready', path: '/tmp/freecadcmd' },
    build123d: { available: true, detail: 'build123d ready', path: '/tmp/python3' },
    directOcct: { available, detail, path: available ? '/tmp/occt' : null },
    mesh: { available: true, detail: 'bundled', path: null },
    recommendedAuthoringContext: {
      engineKind: 'ecky',
      sourceLanguage: 'ecky',
      geometryBackend: 'mesh',
    },
  };
}

test('buildDirectOcctStepStatus returns null for non-mesh bundles', () => {
  const bundle = meshEckyBundle();
  bundle.geometryBackend = 'freecad';

  assert.equal(buildDirectOcctStepStatus(bundle, capabilities(true, 'Direct OCCT ready')), null);
});

test('buildDirectOcctStepStatus reports blocked direct OCCT fast path', () => {
  const status = buildDirectOcctStepStatus(
    meshEckyBundle(),
    capabilities(false, 'Direct OCCT unavailable: missing TKDESTEP'),
  );

  assert.deepEqual(status, {
    label: 'DIRECT OCCT STEP FAST PATH',
    status: 'BLOCKED',
    detail: 'Direct OCCT unavailable: missing TKDESTEP',
    tone: 'blocked',
  });
});

test('buildDirectOcctStepStatus reports ready runtime without STEP artifact', () => {
  const status = buildDirectOcctStepStatus(meshEckyBundle(), capabilities(true, 'Direct OCCT ready'));

  assert.equal(status?.status, 'READY / NO STEP');
  assert.equal(status?.detail, 'Direct OCCT ready; no BRep STEP artifact was produced for this model.');
  assert.equal(status?.tone, 'pending');
});

test('buildDirectOcctStepStatus reports existing STEP artifact', () => {
  const bundle = meshEckyBundle();
  bundle.exportArtifacts = [{ label: 'STEP', format: 'step', path: '/tmp/output.step', role: 'primary' }];

  const status = buildDirectOcctStepStatus(bundle, capabilities(true, 'Direct OCCT ready'));

  assert.equal(status?.status, 'STEP READY');
  assert.equal(status?.detail, 'BRep STEP artifact ready: output.step');
  assert.equal(status?.tone, 'ready');
});

test('buildDirectOcctStepStatus reports unprobed capability', () => {
  const status = buildDirectOcctStepStatus(meshEckyBundle(), null);

  assert.equal(status?.status, 'UNKNOWN');
  assert.equal(status?.detail, 'Direct OCCT capability was not probed.');
  assert.equal(status?.tone, 'pending');
});
