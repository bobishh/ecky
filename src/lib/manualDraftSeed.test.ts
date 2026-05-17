import test from 'node:test';
import assert from 'node:assert/strict';

import { buildFailedDraftSeed } from './manualDraftSeed';
import type { WorkingCopyState } from './stores/workingCopy';
import type { DesignOutput } from './types/domain';

function sampleWorkingDraft(): WorkingCopyState {
  return {
    title: 'Workbench Draft',
    versionName: 'V-existing',
    macroCode: '# default macro',
    macroDialect: 'legacy',
    engineKind: 'freecad',
    sourceLanguage: 'legacyPython',
    geometryBackend: 'freecad',
    uiSpec: {
      fields: [{ type: 'number', key: 'width', label: 'Width', frozen: false }],
    },
    params: { width: 18 },
    postProcessing: null,
    dirty: false,
    sourceVersionId: null,
  };
}

function sampleFailedDesign(overrides: Partial<DesignOutput> = {}): DesignOutput {
  return {
    title: '',
    versionName: '',
    response: '',
    interactionMode: 'design',
    macroCode: 'print("broken bracket")',
    macroDialect: 'legacy',
    engineKind: 'freecad',
    sourceLanguage: 'legacyPython',
    geometryBackend: 'freecad',
    uiSpec: { fields: [] },
    initialParams: {},
    postProcessing: null,
    ...overrides,
  };
}

test('buildFailedDraftSeed keeps failed code and fills missing draft metadata from working copy', () => {
  const seeded = buildFailedDraftSeed(
    sampleFailedDesign(),
    sampleWorkingDraft(),
  );

  assert.equal(seeded.title, 'Workbench Draft');
  assert.equal(seeded.versionName, 'V-existing');
  assert.equal(seeded.macroCode, 'print("broken bracket")');
  assert.equal(seeded.sourceLanguage, 'legacyPython');
  assert.equal(seeded.geometryBackend, 'freecad');
  assert.deepEqual(seeded.uiSpec, sampleWorkingDraft().uiSpec);
  assert.deepEqual(seeded.initialParams, { width: 18 });
});

test('buildFailedDraftSeed preserves explicit failed-design metadata over working copy fallbacks', () => {
  const seeded = buildFailedDraftSeed(
    sampleFailedDesign({
      title: 'Model Retry',
      versionName: 'V-retry',
      macroCode: 'print("retry")',
      sourceLanguage: 'build123d',
      geometryBackend: 'build123d',
      uiSpec: { fields: [] },
      initialParams: { width: 4 },
      postProcessing: null,
    }),
    sampleWorkingDraft(),
  );

  assert.equal(seeded.title, 'Model Retry');
  assert.equal(seeded.versionName, 'V-retry');
  assert.equal(seeded.sourceLanguage, 'build123d');
  assert.equal(seeded.geometryBackend, 'build123d');
  assert.deepEqual(seeded.initialParams, { width: 4 });
});
