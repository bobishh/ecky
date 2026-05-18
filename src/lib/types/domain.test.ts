import assert from 'node:assert/strict';
import test from 'node:test';

import {
  hasActiveLithophaneAttachments,
  normalizeArtifactBundle,
  normalizeConfig,
  normalizeDesignOutput,
  normalizeRuntimeCapabilities,
  normalizeLastDesignSnapshot,
  normalizeMessage,
  normalizePostProcessing,
  normalizeThread,
  toContractDesignOutput,
} from './domain';

test('normalizeDesignOutput resolves legacy defaults', () => {
  const output = normalizeDesignOutput({
    engineKind: 'freecad',
  } as any);

  assert.equal(output.sourceLanguage, 'legacyPython');
  assert.equal(output.geometryBackend, 'freecad');
});

test('normalizeThread does not synthesize authoring context', () => {
  const thread = normalizeThread({
    engine_kind: 'freecad',
    engineKind: 'ecky',
    source_language: 'legacyPython',
    sourceLanguage: 'ecky',
    geometry_backend: 'freecad',
    geometryBackend: 'mesh',
  } as any);

  assert.equal('engineKind' in thread, false);
  assert.equal('sourceLanguage' in thread, false);
  assert.equal('geometryBackend' in thread, false);
});

test('toContractDesignOutput preserves authoring context fields', () => {
  const contract = toContractDesignOutput({
    title: 'Threaded',
    versionName: 'V2',
    response: 'ok',
    interactionMode: 'design',
    macroCode: '(model (part body (box 1 1 1)))',
    macroDialect: 'ecky',
    engineKind: 'ecky',
    sourceLanguage: 'ecky',
    geometryBackend: 'build123d',
    uiSpec: { fields: [] },
    initialParams: {},
    postProcessing: null,
  } as any);

  assert.equal(contract.engineKind, 'ecky');
  assert.equal(contract.sourceLanguage, 'ecky');
  assert.equal(contract.geometryBackend, 'build123d');
});

test('normalizeMessage heals ecky ir output backend from build123d runtime bundle', () => {
  const message = normalizeMessage({
    id: 'm1',
    role: 'assistant',
    content: '',
    status: 'success',
    timestamp: 0,
    output: {
      engineKind: 'ecky',
      sourceLanguage: 'ecky',
      geometryBackend: 'mesh',
      macroCode: '(model (part body (box 1 1 1)))',
      uiSpec: { fields: [] },
      initialParams: {},
    } as any,
    artifactBundle: {
      modelId: 'model',
      sourceKind: 'generated',
      contentHash: 'x',
      fcstdPath: '',
      manifestPath: '',
      previewStlPath: '',
      geometryBackend: 'build123d',
    } as any,
    modelManifest: null,
  } as any);

  assert.equal(message.output?.geometryBackend, 'build123d');
  assert.equal(message.output?.sourceLanguage, 'ecky');
});

test('normalizeArtifactBundle preserves topology target alias ids', () => {
  const bundle = normalizeArtifactBundle({
    modelId: 'model',
    sourceKind: 'generated',
    contentHash: 'x',
    fcstdPath: '',
    manifestPath: '',
    previewStlPath: '',
    edgeTargets: [{
      targetId: 'body:edge:0-0-0_10-0-0',
      durableTargetId: 'body:node:42:edge:0-0-0_10-0-0',
      canonicalTargetId: 'body:edge:0:0-0-0_10-0-0',
      aliasIds: ['body:edge:0:0-0-0_10-0-0'],
    }],
    faceTargets: [{
      targetId: 'body:face:0-0-10:100',
      durableTargetId: 'body:node:42:face:0-0-10:100',
      canonicalTargetId: 'body:face:5:0-0-10:100',
      aliasIds: ['body:face:5:0-0-10:100'],
    }],
  } as any);

  assert.ok(bundle.edgeTargets?.[0]);
  assert.ok(bundle.faceTargets?.[0]);
  assert.equal(bundle.edgeTargets[0].durableTargetId, 'body:node:42:edge:0-0-0_10-0-0');
  assert.equal(bundle.faceTargets[0].durableTargetId, 'body:node:42:face:0-0-10:100');
  assert.equal(bundle.edgeTargets[0].canonicalTargetId, 'body:edge:0:0-0-0_10-0-0');
  assert.equal(bundle.faceTargets[0].canonicalTargetId, 'body:face:5:0-0-10:100');
  assert.deepEqual(bundle.edgeTargets[0].aliasIds, ['body:edge:0:0-0-0_10-0-0']);
  assert.deepEqual(bundle.faceTargets[0].aliasIds, ['body:face:5:0-0-10:100']);
});

test('normalizeMessage preserves durable topology target ids on manifest', () => {
  const message = normalizeMessage({
    id: 'm1',
    role: 'assistant',
    content: '',
    status: 'success',
    timestamp: 0,
    output: null,
    artifactBundle: null,
    modelManifest: {
      modelId: 'model',
      sourceKind: 'generated',
      contentHash: 'x',
      fcstdPath: '',
      manifestPath: '',
      previewStlPath: '',
      document: {
        documentName: 'Doc',
        documentLabel: 'Doc',
        sourcePath: null,
        objectCount: 1,
        warnings: [],
      },
      parts: [],
      parameterGroups: [],
      controlPrimitives: [],
      controlRelations: [],
      controlViews: [],
      advisories: [],
      selectionTargets: [{
        targetId: 'body:edge:0-0-0_10-0-0',
        durableTargetId: 'body:node:42:edge:0-0-0_10-0-0',
        canonicalTargetId: 'body:edge:0:0-0-0_10-0-0',
        aliasIds: ['legacy-edge'],
        partId: 'body',
        viewerNodeId: 'Body001',
        label: 'Body Edge',
        kind: 'edge',
        editable: true,
        parameterKeys: [],
        primitiveIds: [],
        viewIds: [],
      }],
      measurementAnnotations: [],
      warnings: [],
      enrichmentState: { status: 'none', proposals: [] },
    } as any,
  } as any);

  assert.equal(
    message.modelManifest?.selectionTargets?.[0]?.durableTargetId,
    'body:node:42:edge:0-0-0_10-0-0',
  );
});

test('normalizeLastDesignSnapshot heals ecky ir output backend from build123d runtime bundle', () => {
  const snapshot = normalizeLastDesignSnapshot({
    design: {
      engineKind: 'ecky',
      sourceLanguage: 'ecky',
      geometryBackend: 'mesh',
      macroCode: '(model (part body (box 1 1 1)))',
      uiSpec: { fields: [] },
      initialParams: {},
    },
    artifactBundle: {
      modelId: 'model',
      sourceKind: 'generated',
      contentHash: 'x',
      fcstdPath: '',
      manifestPath: '',
      previewStlPath: '',
      geometryBackend: 'build123d',
    },
    modelManifest: null,
  } as any);

  assert.equal(snapshot?.design?.geometryBackend, 'build123d');
  assert.equal(snapshot?.design?.sourceLanguage, 'ecky');
});

test('normalizeRuntimeCapabilities accepts legacy mesh alias and returns mesh only', () => {
  const normalized = normalizeRuntimeCapabilities({
    freecad: { available: true, detail: 'FreeCAD ready', path: '/tmp/freecadcmd' },
    build123d: { available: true, detail: 'BUILD123D ready', path: '/tmp/python3' },
    directOcct: { available: true, detail: 'Direct OCCT ready', path: '/tmp/include' },
    eckyRust: { available: true, detail: 'NATIVE ready', path: '/tmp/mesh' },
    recommendedAuthoringContext: {
      engineKind: 'ecky',
      sourceLanguage: 'ecky',
      geometryBackend: 'mesh',
    },
  } as any);

  assert.equal(normalized.directOcct.detail, 'Direct OCCT ready');
  assert.equal(normalized.mesh.detail, 'NATIVE ready');
  assert.equal(normalized.recommendedAuthoringContext.engineKind, 'ecky');
  assert.equal(normalized.recommendedAuthoringContext.sourceLanguage, 'ecky');
  assert.equal(normalized.recommendedAuthoringContext.geometryBackend, 'mesh');
});

test('normalizeRuntimeCapabilities gives internal direct OCCT safe fallback', () => {
  const normalized = normalizeRuntimeCapabilities({
    freecad: { available: false, detail: 'missing' },
    build123d: { available: false, detail: 'missing' },
    mesh: { available: true, detail: 'bundled' },
  } as any);

  assert.equal(normalized.directOcct.available, false);
  assert.equal(normalized.directOcct.detail, 'Unavailable');
});

test('normalizePostProcessing lifts legacy displacement into a lithophane attachment', () => {
  const normalized = normalizePostProcessing({
    displacement: {
      imageParam: 'image_path',
      projection: 'planar',
      depthMm: 2.5,
      invert: true,
    },
  });

  assert.ok(normalized);
  assert.equal(normalized?.lithophaneAttachments?.length, 1);
  assert.deepEqual(normalized?.lithophaneAttachments?.[0], {
    id: 'legacy-image-path',
    enabled: true,
    source: { kind: 'param', imageParam: 'image_path' },
    targetPartId: '',
    placement: {
      mode: 'partSidePatch',
      side: 'front',
      projection: 'planar',
      widthMm: 0,
      heightMm: 0,
      offsetXMm: 0,
      offsetYMm: 0,
      rotationDeg: 0,
      overflowMode: 'contain',
      bleedMarginMm: 0,
    },
    relief: {
      depthMm: 2.5,
      invert: true,
    },
    color: {
      mode: 'mono',
      channelThicknessMm: 0.4,
    },
  });
});

test('hasActiveLithophaneAttachments ignores disabled attachments', () => {
  assert.equal(
    hasActiveLithophaneAttachments({
      lithophaneAttachments: [
        {
          id: 'off',
          enabled: false,
          source: { kind: 'file', imagePath: '/tmp/x.png' },
          targetPartId: '',
          placement: { mode: 'partSidePatch', side: 'front', projection: 'auto' },
          relief: { depthMm: 1, invert: false },
          color: { mode: 'mono', channelThicknessMm: 0.4 },
        },
      ],
    }),
    false,
  );
});

test('normalizeConfig defaults STT language to en-US when voice config is missing', () => {
  const config = normalizeConfig({
    engines: [],
    selectedEngineId: '',
  } as any);

  assert.equal(config.voice.sttLanguageCode, 'en-US');
});

test('normalizeConfig preserves configured STT language code', () => {
  const config = normalizeConfig({
    engines: [],
    selectedEngineId: '',
    voice: { sttLanguageCode: 'ru-RU' },
  } as any);

  assert.equal(config.voice.sttLanguageCode, 'ru-RU');
});

test('normalizeConfig preserves CAD text font path', () => {
  const config = normalizeConfig({
    engines: [],
    selectedEngineId: '',
    cadTextFontPath: '/System/Library/Fonts/Supplemental/Arial Black.ttf',
  } as any);

  assert.equal(config.cadTextFontPath, '/System/Library/Fonts/Supplemental/Arial Black.ttf');
});
