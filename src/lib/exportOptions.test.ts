import assert from 'node:assert/strict';
import test from 'node:test';

import type { ArtifactBundle, RuntimeCapabilities } from './types/domain';
import {
  buildExportChooserOptions,
  buildExportDefaultNames,
  buildMultipartExportParts,
  hasMultipartExportAssets,
} from './exportOptions';

function sampleBundle(viewerAssetCount: number): ArtifactBundle {
  return {
    schemaVersion: 1,
    modelId: 'model-1',
    sourceKind: 'generated',
    contentHash: 'hash',
    artifactVersion: 1,
    fcstdPath: '/tmp/model.FCStd',
    manifestPath: '/tmp/model.json',
    macroPath: '/tmp/model.FCMacro',
    previewStlPath: '/tmp/model.stl',
    viewerAssets: Array.from({ length: viewerAssetCount }, (_, index) => ({
      partId: `part-${index + 1}`,
      nodeId: `node-${index + 1}`,
      objectName: `Object${index + 1}`,
      label: index === 0 ? 'Shade Body' : 'Trim Ring',
      path: `/tmp/part-${index + 1}.stl`,
      format: 'stl',
    })),
    exportArtifacts: [],
  };
}

function capabilities(directOcctAvailable: boolean, detail: string): RuntimeCapabilities {
  return {
    freecad: { available: true, detail: 'FreeCAD ready', path: '/tmp/freecadcmd' },
    build123d: { available: true, detail: 'build123d ready', path: '/tmp/python3' },
    directOcct: { available: directOcctAvailable, detail, path: directOcctAvailable ? '/tmp/occt' : null },
    mesh: { available: true, detail: 'bundled', path: null },
    recommendedAuthoringContext: {
      engineKind: 'ecky',
      sourceLanguage: 'ecky',
      geometryBackend: 'mesh',
    },
  };
}

test('buildExportChooserOptions shows only STL and FCStd for single-part models', () => {
  const options = buildExportChooserOptions(sampleBundle(1));

  assert.deepEqual(
    options.map((option) => option.id),
    ['stl', 'fcstd', 'step'],
  );
  assert.equal(options[2]?.disabled, true);
  assert.equal(options[2]?.disabledReason, 'STEP artifact is not present in this model bundle.');
});

test('buildExportChooserOptions reports direct OCCT blocker for mesh STEP export', () => {
  const bundle = sampleBundle(1);
  bundle.sourceLanguage = 'ecky';
  bundle.geometryBackend = 'mesh';

  const step = buildExportChooserOptions(
    bundle,
    capabilities(false, 'Direct OCCT unavailable: missing TKDESTEP'),
  ).find((option) => option.id === 'step');

  assert.equal(step?.disabled, true);
  assert.equal(
    step?.disabledReason,
    'STEP unavailable for Ecky Native render: Direct OCCT unavailable: missing TKDESTEP',
  );
});

test('buildExportChooserOptions reports mesh-only bundle when direct OCCT is available but no STEP exists', () => {
  const bundle = sampleBundle(1);
  bundle.sourceLanguage = 'ecky';
  bundle.geometryBackend = 'mesh';

  const step = buildExportChooserOptions(
    bundle,
    capabilities(true, 'Direct OCCT ready'),
  ).find((option) => option.id === 'step');

  assert.equal(step?.disabled, true);
  assert.equal(
    step?.disabledReason,
    'STEP unavailable for Ecky Native render: no BRep STEP artifact was produced.',
  );
});

test('buildExportChooserOptions shows multipart options first for multipart models', () => {
  const options = buildExportChooserOptions(sampleBundle(2));

  assert.deepEqual(
    options.map((option) => option.id),
    ['3mf', 'multipartStlZip', 'stl', 'fcstd', 'step'],
  );
  assert.match(options[2]?.subtitle ?? '', /Flattened single-mesh export/);
});

test('buildExportChooserOptions enables STEP when a STEP artifact exists', () => {
  const bundle = sampleBundle(1);
  bundle.exportArtifacts = [
    { label: 'Neutral CAD', format: 'step', path: '/tmp/model.step', role: 'cad-exchange' },
  ];

  const step = buildExportChooserOptions(bundle).find((option) => option.id === 'step');

  assert.equal(step?.disabled, false);
  assert.equal(step?.disabledReason, undefined);
});

test('buildMultipartExportParts preserves viewer asset order and labels', () => {
  const parts = buildMultipartExportParts(sampleBundle(2));

  assert.deepEqual(parts, [
    {
      label: 'Shade Body',
      path: '/tmp/part-1.stl',
      objectName: 'Object1',
      partId: 'part-1',
      displayColor: null,
    },
    {
      label: 'Trim Ring',
      path: '/tmp/part-2.stl',
      objectName: 'Object2',
      partId: 'part-2',
      displayColor: null,
    },
  ]);
});

test('buildExportDefaultNames sanitizes the model title into stable filenames', () => {
  assert.deepEqual(buildExportDefaultNames('Bulb Lamp Shade / Final'), {
    threeMf: 'bulb-lamp-shade-final.3mf',
    multipartStlZip: 'bulb-lamp-shade-final-parts.zip',
    stl: 'bulb-lamp-shade-final.stl',
    fcstd: 'bulb-lamp-shade-final.FCStd',
    step: 'bulb-lamp-shade-final.step',
  });
});

test('hasMultipartExportAssets requires more than one viewer asset', () => {
  assert.equal(hasMultipartExportAssets(sampleBundle(0)), false);
  assert.equal(hasMultipartExportAssets(sampleBundle(1)), false);
  assert.equal(hasMultipartExportAssets(sampleBundle(2)), true);
});
