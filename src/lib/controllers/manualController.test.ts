import assert from 'node:assert/strict';
import test from 'node:test';

import {
  recordParamsChanged,
  resolveManualRenderRoute,
  shouldPreserveWorkingCopyMacroDraft,
} from './manualController';
import {
  clearSessionActivityEvents,
  currentSessionActivityEvents,
} from '../stores/sessionActivityStore';

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

test('params change emits exactly one session event per action', () => {
  clearSessionActivityEvents();

  recordParamsChanged({
    threadId: 'thread-1',
    versionId: 'version-1',
    before: { width: 10, height: 20, depth: 5 },
    after: { width: 12, height: 22, depth: 6 },
    persist: false,
  });

  const events = currentSessionActivityEvents();
  assert.equal(events.length, 1, 'one event per params action, not per key');
  assert.equal(events[0].kind, 'params_changed');
  assert.equal(events[0].diffs?.length, 3, 'all changed keys ride one event');

  clearSessionActivityEvents();
});

test('no-op params change emits no session event', () => {
  clearSessionActivityEvents();

  recordParamsChanged({
    threadId: 'thread-1',
    versionId: 'version-1',
    before: { width: 10 },
    after: { width: 10 },
    persist: true,
  });

  assert.equal(currentSessionActivityEvents().length, 0);

  clearSessionActivityEvents();
});

test('repeated params actions emit one event each with unique ids', () => {
  clearSessionActivityEvents();

  recordParamsChanged({
    threadId: 'thread-1',
    versionId: 'version-1',
    before: { width: 10 },
    after: { width: 11 },
    persist: false,
  });
  recordParamsChanged({
    threadId: 'thread-1',
    versionId: 'version-1',
    before: { width: 11 },
    after: { width: 12 },
    persist: false,
  });

  const events = currentSessionActivityEvents();
  assert.equal(events.length, 2, 'two actions, two events');
  assert.notEqual(events[0].id, events[1].id, 'ids stay unique for keyed rendering');

  clearSessionActivityEvents();
});
