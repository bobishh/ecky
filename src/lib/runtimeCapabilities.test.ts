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
  return {
    freecad: { available: false, detail: 'FreeCAD missing', path: null },
    build123d: { available: true, detail: 'BUILD123D ready', path: '/tmp/python3' },
    eckyRust: { available: true, detail: 'bundled', path: null },
    recommendedAuthoringContext: {
      engineKind: 'eckyIrV0',
      sourceLanguage: 'eckyIrV0',
      geometryBackend: 'build123d',
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

test('capabilityForAuthoringContext routes legacy/freecad and ecky rust correctly', () => {
  const capabilities = sampleCapabilities({
    freecad: { available: true, detail: 'FreeCAD ready', path: '/tmp/freecadcmd' },
  });

  assert.equal(
    capabilityForAuthoringContext(capabilities, 'legacyPython', 'freecad')?.detail,
    'FreeCAD ready',
  );
  assert.equal(
    capabilityForAuthoringContext(capabilities, 'eckyIrV0', 'eckyRust')?.detail,
    'bundled',
  );
});

test('repairDefaultAuthoringContext keeps valid persisted default', () => {
  const config = sampleConfig({
    defaultEngineKind: 'eckyIrV0',
    defaultSourceLanguage: 'eckyIrV0',
    defaultGeometryBackend: 'build123d',
  });
  const capabilities = sampleCapabilities();

  const result = repairDefaultAuthoringContext(config, capabilities);

  assert.equal(result.repaired, false);
  assert.equal(result.config.defaultGeometryBackend, 'build123d');
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
      engineKind: 'eckyIrV0',
      sourceLanguage: 'eckyIrV0',
      geometryBackend: 'build123d',
    },
  );
});
