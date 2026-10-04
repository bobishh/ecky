import assert from 'node:assert/strict';
import test from 'node:test';

import {
  authoringContextFromConfig,
  capabilityForAuthoringContext,
  repairDefaultAuthoringContext,
  resolveActiveAuthoringContext,
} from './runtimeCapabilities';
import type { AppConfig, ArtifactBundle, ModelManifest, RuntimeCapabilities } from './types/domain';

function sampleConfig(overrides: Partial<AppConfig> = {}): AppConfig {
  return {
    engines: [],
    selectedEngineId: '',
    freecadCmd: '',
    cadTextFontPath: '',
    projectsRoot: '',
    freecadLibraryRoots: [],
    assets: [],
    microwave: null,
    voice: {
      sttLanguageCode: 'en-US',
    },
    mcp: {
      port: null,
      maxSessions: null,
      mode: 'passive',
      primaryAgentId: null,
      promptTimeoutSecs: 1800,
      eckyAstAuthoring: false,
      autoAgents: [],
    },
    femCompute: {
      quality: 'balanced',
      maximumWallTimeMinutes: 30,
      maximumMemoryMiB: 8192,
      threadCount: 0,
    },
    hasSeenOnboarding: true,
    connectionType: null,
    providerModels: { codex: '', agy: '' },
  jevClassifier: { enabled: false, apiKey: '' },
    defaultEngineKind: 'freecad',
    defaultSourceLanguage: 'legacyPython',
    defaultGeometryBackend: 'freecad',
    maxGenerationAttempts: 3,
    maxVerifyAttempts: 0,
    ...overrides,
  };
}

function sampleCapabilities(overrides: Partial<RuntimeCapabilities> = {}): RuntimeCapabilities {
  const mesh = { available: true, detail: 'NATIVE ready', path: null };
  return {
    freecad: { available: false, detail: 'FreeCAD missing', path: null },
    build123d: { available: true, detail: 'BUILD123D ready', path: '/tmp/python3' },
    directOcct: { available: false, detail: 'Direct OCCT unavailable', path: null },
    mesh,
    recommendedAuthoringContext: {
      engineKind: 'ecky',
      sourceLanguage: 'ecky',
      geometryBackend: 'mesh',
    },
    ...overrides,
  };
}

test('authoringContextFromConfig migrates persisted build123d defaults to native', () => {
  assert.deepEqual(
    authoringContextFromConfig(
      sampleConfig({
        defaultEngineKind: 'build123d',
        defaultSourceLanguage: 'build123d',
        defaultGeometryBackend: 'build123d',
      }),
    ),
    {
      engineKind: 'ecky',
      sourceLanguage: 'ecky',
      geometryBackend: 'mesh',
    },
  );
});

test('capabilityForAuthoringContext routes legacy/freecad and ecky mesh correctly', () => {
  const capabilities = sampleCapabilities({
    freecad: { available: true, detail: 'FreeCAD ready', path: '/tmp/freecadcmd' },
  });

  assert.equal(
    capabilityForAuthoringContext(capabilities, 'legacyPython', 'freecad')?.detail,
    'FreeCAD ready',
  );
  assert.equal(
    capabilityForAuthoringContext(capabilities, 'ecky', 'mesh')?.detail,
    'NATIVE ready',
  );
  assert.equal(
    capabilityForAuthoringContext(capabilities, 'ecky', 'mesh')?.detail,
    'NATIVE ready',
  );
});

test('resolveActiveAuthoringContext prefers selected version artifact metadata over config defaults', () => {
  const context = resolveActiveAuthoringContext({
    config: sampleConfig(),
    activeVersionMessage: {
      output: null,
      modelManifest: null,
      artifactBundle: {
        modelId: 'selected-model',
        sourceKind: 'generated',
        sourceLanguage: 'ecky',
        geometryBackend: 'build123d',
        contentHash: 'hash',
        fcstdPath: '',
        manifestPath: '',
        modelStlPath: '',
      } as ArtifactBundle,
    },
    sessionArtifactBundle: null,
    sessionModelManifest: null,
  });

  assert.deepEqual(context, {
    engineKind: 'ecky',
    sourceLanguage: 'ecky',
    geometryBackend: 'mesh',
  });
});

test('resolveActiveAuthoringContext falls back to session runtime before config defaults', () => {
  const context = resolveActiveAuthoringContext({
    config: sampleConfig(),
    activeVersionMessage: null,
    sessionArtifactBundle: null,
    sessionModelManifest: {
      modelId: 'session-model',
      sourceKind: 'generated',
      sourceLanguage: 'build123d',
      geometryBackend: 'build123d',
      taggedAnchors: {},
      analysisDeclarations: [],
      document: {
        documentName: 'Session',
        documentLabel: 'Session',
        objectCount: 1,
        warnings: [],
      },
    } as ModelManifest,
  });

  assert.deepEqual(context, {
    engineKind: 'ecky',
    sourceLanguage: 'ecky',
    geometryBackend: 'mesh',
  });
});

test('repairDefaultAuthoringContext migrates removed build123d default', () => {
  const config = sampleConfig({
    defaultEngineKind: 'ecky',
    defaultSourceLanguage: 'ecky',
    defaultGeometryBackend: 'build123d',
  });
  const capabilities = sampleCapabilities();

  const result = repairDefaultAuthoringContext(config, capabilities);

  assert.equal(result.repaired, true);
  assert.equal(result.config.defaultGeometryBackend, 'mesh');
});

test('repairDefaultAuthoringContext never selects direct OCCT while internal only', () => {
  const result = repairDefaultAuthoringContext(
    sampleConfig(),
    sampleCapabilities({
      build123d: { available: false, detail: 'missing', path: null },
      directOcct: { available: true, detail: 'Direct OCCT ready', path: '/tmp/include' },
      recommendedAuthoringContext: {
        engineKind: 'ecky',
        sourceLanguage: 'ecky',
        geometryBackend: 'mesh',
      },
    }),
  );

  assert.equal(result.config.defaultGeometryBackend, 'mesh');
});

test('repairDefaultAuthoringContext falls back to recommended context when freecad default is unavailable', () => {
  const result = repairDefaultAuthoringContext(sampleConfig(), sampleCapabilities());

  assert.equal(result.repaired, true);
  assert.deepEqual(
    {
      engineKind: result.config.defaultEngineKind,
      sourceLanguage: result.config.defaultSourceLanguage,
      geometryBackend: result.config.defaultGeometryBackend,
    },
    {
      engineKind: 'ecky',
      sourceLanguage: 'ecky',
      geometryBackend: 'mesh',
    },
  );
});
