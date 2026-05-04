import assert from 'node:assert/strict';
import test from 'node:test';

import { buildSketchAcceptedCadRow } from './sketchAcceptedCad';
import type { BrepHiddenLineProjectionResponse } from './tauri/contracts';
import type { ArtifactBundle } from './types/domain';

const artifactBundle: ArtifactBundle = {
  modelId: 'model-1',
  sourceKind: 'generated',
  engineKind: 'freecad',
  sourceLanguage: 'ecky',
  geometryBackend: 'freecad',
  contentHash: 'hash',
  artifactVersion: 1,
  fcstdPath: '/tmp/model.FCStd',
  manifestPath: '/tmp/manifest.json',
  macroPath: '/tmp/source.ecky',
  previewStlPath: '/tmp/preview.stl',
  viewerAssets: [],
};

const passingProjection: BrepHiddenLineProjectionResponse = {
  modelId: 'model-1',
  sourceArtifactPath: '/tmp/model.FCStd',
  views: [
    { view: 'front', direction: [0, -1, 0], visibleEdges: [], hiddenEdges: [] },
    { view: 'top', direction: [0, 0, -1], visibleEdges: [], hiddenEdges: [] },
    { view: 'side', direction: [-1, 0, 0], visibleEdges: [], hiddenEdges: [] },
  ],
  warnings: [],
  validation: {
    passed: true,
    issues: [],
    evidence: ['backend BRep/sketch validation passed'],
  },
};

test('buildSketchAcceptedCadRow passes only after explicit BRep/sketch validation passes', () => {
  assert.deepEqual(
    buildSketchAcceptedCadRow({
      artifactBundle,
      hiddenLineResponse: passingProjection,
      hiddenLineErrorText: '',
      hiddenLineLoading: false,
    }),
    {
      id: 'acceptedCad',
      label: 'Accepted CAD',
      status: 'pass',
      detail: 'Accepted BRep; 3 views validated; model.FCStd; backend BRep/sketch validation passed',
    },
  );
});

test('buildSketchAcceptedCadRow fails with raw BRep/sketch issue text', () => {
  const row = buildSketchAcceptedCadRow({
    artifactBundle,
    hiddenLineResponse: {
      ...passingProjection,
      validation: {
        passed: false,
        issues: [
          {
            sketchId: 'sketch-front',
            primitiveId: 'primitive-front',
            severity: 'error',
            message: 'raw BREP/SKETCH bounds mismatch: front sketch bounds x=10..60; OCCT bounds x=0..80',
          },
        ],
        evidence: [],
      },
    },
    hiddenLineErrorText: '',
    hiddenLineLoading: false,
  });

  assert.equal(row?.status, 'fail');
  assert.match(row?.detail ?? '', /raw BREP\/SKETCH bounds mismatch/);
});

test('buildSketchAcceptedCadRow keeps mesh preview pending instead of accepting it as CAD', () => {
  const row = buildSketchAcceptedCadRow({
    artifactBundle: { ...artifactBundle, geometryBackend: 'mesh', fcstdPath: '', exportArtifacts: [] },
    hiddenLineResponse: null,
    hiddenLineErrorText: '',
    hiddenLineLoading: false,
  });

  assert.deepEqual(row, {
    id: 'acceptedCad',
    label: 'Accepted CAD',
    status: 'pending',
    detail: 'Preview artifact only; accepted CAD requires exact BRep/STEP validation.',
  });
});
