import assert from 'node:assert/strict';
import test from 'node:test';

import { resolveParamApplySource } from './paramApplySource';

test('forced code resolves without reading panel macro code', () => {
  const result = resolveParamApplySource({
    forcedCode: 'forced()',
    workingMacroCode: 'working()',
    panelVersionId: 'stale-panel-version',
    sourceVersionId: 'version-1',
    activeVersionId: 'version-1',
  });

  assert.equal(result.ok, true);
  assert.equal(result.code, 'forced()');
  assert.equal(result.source, 'forced');
  assert.equal(result.targetVersionId, 'version-1');
});

test('working copy macro resolves when forced code is absent', () => {
  const result = resolveParamApplySource({
    forcedCode: null,
    workingMacroCode: 'working()',
    panelVersionId: 'version-2',
    sourceVersionId: 'version-2',
    activeVersionId: 'version-2',
  });

  assert.equal(result.ok, true);
  assert.equal(result.code, 'working()');
  assert.equal(result.source, 'workingCopy');
  assert.equal(result.targetVersionId, 'version-2');
});

test('stale panel/source version mismatch blocks macro selection', () => {
  const result = resolveParamApplySource({
    forcedCode: null,
    workingMacroCode: 'working()',
    panelVersionId: 'old-version',
    sourceVersionId: 'new-version',
    activeVersionId: 'new-version',
  });

  assert.equal(result.ok, false);
  assert.equal(result.reason, 'stale-panel-source-version-mismatch');
  assert.equal(result.panelVersionId, 'old-version');
  assert.equal(result.sourceVersionId, 'new-version');
});

test('missing macro code returns explicit no-source result', () => {
  const result = resolveParamApplySource({
    forcedCode: '',
    workingMacroCode: '',
    panelVersionId: 'version-3',
    sourceVersionId: 'version-3',
    activeVersionId: 'version-3',
  });

  assert.equal(result.ok, false);
  assert.equal(result.reason, 'missing-macro-code');
  assert.equal(result.targetVersionId, 'version-3');
});
