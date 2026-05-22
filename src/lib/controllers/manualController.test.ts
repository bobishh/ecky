import assert from 'node:assert/strict';
import test from 'node:test';

import { resolveManualRenderRoute, shouldPreserveWorkingCopyMacroDraft } from './manualController';

test('param commit keeps current macro code draft when committed macro differs', () => {
  const preserve = shouldPreserveWorkingCopyMacroDraft(
    { macroCode: 'draft macro();', dirty: true },
    'committed macro();',
  );

  assert.equal(preserve, true);
});

test('param commit does not preserve macro draft when working copy matches commit', () => {
  const preserve = shouldPreserveWorkingCopyMacroDraft(
    { macroCode: 'macro();', dirty: false },
    'macro();',
  );

  assert.equal(preserve, false);
});

test('manual route renders ecky source on configured geometry backend', () => {
  const route = resolveManualRenderRoute({
    code: '(model (box :size [10 10 10]))',
    configDefaultGeometryBackend: 'mesh',
    workingMacroDialect: 'build123d',
    workingSourceLanguage: 'build123d',
  });

  assert.deepEqual(route, {
    macroDialect: 'ecky',
    geometryBackend: 'mesh',
  });
});

test('manual route keeps build123d python on build123d backend', () => {
  const route = resolveManualRenderRoute({
    code: 'from build123d import *\nBox(1, 2, 3)',
    configDefaultGeometryBackend: 'mesh',
    workingMacroDialect: 'legacy',
    workingSourceLanguage: 'legacyPython',
  });

  assert.deepEqual(route, {
    macroDialect: 'build123d',
    geometryBackend: 'build123d',
  });
});

test('manual route keeps raw python macro off ecky config backend', () => {
  const route = resolveManualRenderRoute({
    code: 'import FreeCAD\nprint("raw macro")',
    configDefaultGeometryBackend: 'mesh',
    workingMacroDialect: 'legacy',
    workingSourceLanguage: 'legacyPython',
  });

  assert.deepEqual(route, {
    macroDialect: 'legacy',
    geometryBackend: 'freecad',
  });
});
