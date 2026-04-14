import assert from 'node:assert/strict';
import test from 'node:test';
import { buildAuthoringDigest, buildManifestDigest, buildParamsDigest, buildUiSpecDigest } from './llmContextDigest';

test('llmContextDigest summarizes ui fields without full json', () => {
    const digest = buildUiSpecDigest({
      fields: [
        { type: 'number', key: 'width', label: 'Width', min: 10, max: 200, frozen: false },
        { type: 'select', key: 'mode', label: 'Mode', options: [{ label: 'A', value: 'a' }], frozen: false },
      ],
    });
    assert.ok(digest?.includes('UI fields: 2'));
    assert.ok(digest?.includes('width: number 10..200'));
    assert.ok(digest?.includes('mode: select (1 options)'));
    assert.ok(!digest?.includes('{'));
});

test('llmContextDigest summarizes params compactly', () => {
    const digest = buildParamsDigest({ width: 120, enabled: true, name: 'basket' });
    assert.ok(digest?.includes('Current params: 3'));
    assert.ok(digest?.includes('width = 120'));
    assert.ok(digest?.includes('enabled = true'));
    assert.ok(digest?.includes('name = "basket"'));
});

test('llmContextDigest summarizes manifest parts without asset paths', () => {
    const digest = buildManifestDigest({
      modelId: 'm1',
      schemaVersion: 1,
      sourceKind: 'generated',
      engineKind: 'freecad',
      sourceLanguage: 'legacyPython',
      geometryBackend: 'freecad',
      document: { documentName: 'doc', documentLabel: 'doc', objectCount: 1, warnings: [] },
      parts: [
        {
          partId: 'basket',
          freecadObjectName: 'Body',
          label: 'Basket',
          kind: 'solid',
          semanticRole: 'body',
          viewerAssetPath: '/tmp/basket.stl',
          viewerNodeIds: [],
          parameterKeys: [],
          editable: true,
          bounds: { xMin: 0, yMin: 0, zMin: 0, xMax: 120, yMax: 80, zMax: 60 },
          volume: 10,
          area: 20,
        },
      ],
      parameterGroups: [],
      controlPrimitives: [],
      controlRelations: [],
      controlViews: [],
      advisories: [],
      selectionTargets: [],
      measurementAnnotations: [],
      warnings: [],
      enrichmentState: { status: 'none', proposals: [] },
    });
    assert.ok(digest?.includes('Model parts: 1'));
    assert.ok(digest?.includes('Basket [solid] role=body size≈120×80×60 mm'));
    assert.ok(!digest?.includes('/tmp/basket.stl'));
});

test('llmContextDigest builds a combined authoring digest', () => {
    const digest = buildAuthoringDigest({
      title: 'Tray',
      versionName: 'V3',
      sourceLanguage: 'eckyIrV0',
      uiSpec: { fields: [{ type: 'checkbox', key: 'lip', label: 'Lip', frozen: false }] },
      params: { lip: true },
      modelManifest: null,
    });
    assert.ok(digest.includes('CURRENT WORKING SNAPSHOT'));
    assert.ok(digest.includes('Tray [V3] (eckyIrV0)'));
    assert.ok(digest.includes('UI fields: 1'));
    assert.ok(digest.includes('Current params: 1'));
});
