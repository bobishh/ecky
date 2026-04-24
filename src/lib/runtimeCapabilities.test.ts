import assert from 'node:assert/strict';
import test from 'node:test';

import {
  authoringContextFromConfig,
  capabilityForAuthoringContext,
  repairDefaultAuthoringContext,
} from './runtimeCapabilities';
import type { AppConfig, RuntimeCapabilities } from './types/domain';

function sampleConfig(overrides: Partial<AppConfig> = {}): AppConfig {
  return {
    engines: [],
    selectedEngineId: '',
    freecadCmd: '',
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
      autoAgents: [],
    },
    hasSeenOnboarding: true,
    connectionType: null,
    defaultEngineKind: 'freecad',
    defaultSourceLanguage: 'legacyPython',
    defaultGeometryBackend: 'freecad',
    maxGenerationAttempts: 3,
    maxVerifyAttempts: 0,
    ...overrides,
  };
}

function sampleCapabilities(overrides: Partial<RuntimeCapabilities> = {}): RuntimeCapabilities {
  const mesh = { available: true, detail: 'MESH ready', path: null };
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

test('authoringContextFromConfig mirrors persisted defaults', () => {
  assert.deepEqual(
    authoringContextFromConfig(
      sampleConfig({
        defaultEngineKind: 'build123d',
        defaultSourceLanguage: 'build123d',
        defaultGeometryBackend: 'build123d',
      }),
    ),
    {
      engineKind: 'build123d',
      sourceLanguage: 'build123d',
      geometryBackend: 'build123d',
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
    'MESH ready',
  );
  assert.equal(
    capabilityForAuthoringContext(capabilities, 'ecky', 'mesh')?.detail,
    'MESH ready',
  );
});

test('repairDefaultAuthoringContext keeps valid persisted default', () => {
  const config = sampleConfig({
    defaultEngineKind: 'ecky',
    defaultSourceLanguage: 'ecky',
    defaultGeometryBackend: 'build123d',
  });
  const capabilities = sampleCapabilities();

  const result = repairDefaultAuthoringContext(config, capabilities);

  assert.equal(result.repaired, false);
  assert.equal(result.config.defaultGeometryBackend, 'build123d');
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
