import assert from 'node:assert/strict';
import test from 'node:test';

import { get } from 'svelte/store';

import {
  ALL_WINDOW_IDS,
  _mergeDbLayout,
  _resetWindowStoreForTest,
  bringToFront,
  fitVisibleWindowsToViewport,
  showWindow,
  windowRegistry,
  windowStore,
} from './windowStore';

test('showWindow opens hidden window, clears minimized state, and raises z order', () => {
  _resetWindowStoreForTest();

  showWindow('dialogue');
  let state = get(windowStore);
  const firstZ = state.dialogue.z;
  assert.equal(state.dialogue.visible, true);
  assert.equal(state.dialogue.minimized, false);

  showWindow('params');
  state = get(windowStore);
  const paramsZ = state.params.z;
  assert.equal(state.params.visible, true);
  assert.ok(paramsZ > firstZ);

  showWindow('dialogue');
  state = get(windowStore);
  assert.equal(state.dialogue.visible, true);
  assert.equal(state.dialogue.minimized, false);
  assert.ok(state.dialogue.z > paramsZ);

  _resetWindowStoreForTest();
});

test('bringToFront makes the clicked visible window the focused top window', () => {
  _resetWindowStoreForTest();

  showWindow('projects');
  showWindow('params');
  let state = get(windowStore);
  assert.equal(state.params.active, true);
  assert.equal(state.projects.active, false);
  const paramsZ = state.params.z;

  bringToFront('projects');
  state = get(windowStore);
  assert.equal(state.projects.active, true);
  assert.equal(state.params.active, false);
  assert.ok(state.projects.z > paramsZ);

  _resetWindowStoreForTest();
});

test('activity window is registered and can be opened', () => {
  _resetWindowStoreForTest();

  assert.ok(ALL_WINDOW_IDS.includes('activity'));
  assert.equal(windowRegistry.activity.title, 'Session Activity');

  showWindow('activity');
  const state = get(windowStore);
  assert.equal(state.activity.visible, true);
  assert.equal(state.activity.minimized, false);
  assert.ok(state.activity.width >= windowRegistry.activity.minSize.width);

  _resetWindowStoreForTest();
});

test('capture window is registered and can be opened', () => {
  _resetWindowStoreForTest();

  assert.ok(ALL_WINDOW_IDS.includes('capture'));
  assert.equal(windowRegistry.capture.title, 'Capture Session');

  showWindow('capture');
  const state = get(windowStore);
  assert.equal(state.capture.visible, true);
  assert.equal(state.capture.minimized, false);
  assert.ok(state.capture.width >= windowRegistry.capture.minSize.width);

  _resetWindowStoreForTest();
});

test('docs window is registered and can be opened', () => {
  _resetWindowStoreForTest();

  assert.ok(ALL_WINDOW_IDS.includes('docs'));
  assert.equal(windowRegistry.docs.title, 'Ecky IR Docs');

  showWindow('docs');
  const state = get(windowStore);
  assert.equal(state.docs.visible, true);
  assert.equal(state.docs.minimized, false);
  assert.ok(state.docs.width >= windowRegistry.docs.minSize.width);

  _resetWindowStoreForTest();
});

test('saved docs geometry restores without reopening the transient docs window', () => {
  _resetWindowStoreForTest();

  const restored = _mergeDbLayout({
    schemaVersion: 1,
    rememberLayout: true,
    windows: {
      docs: { visible: true, minimized: false, x: 170, y: 100, width: 1000, height: 700, z: 8 },
    },
  });

  assert.equal(restored.docs.visible, false);
  assert.deepEqual(
    { x: restored.docs.x, y: restored.docs.y, width: restored.docs.width, height: restored.docs.height },
    { x: 170, y: 100, width: 1000, height: 700 },
  );

  _resetWindowStoreForTest();
});

test('library window is registered independently from projects', () => {
  _resetWindowStoreForTest();

  assert.ok(ALL_WINDOW_IDS.includes('library'));
  assert.equal(windowRegistry.library.title, 'Library');

  showWindow('library');
  const state = get(windowStore);
  assert.equal(state.library.visible, true);
  assert.equal(state.projects.visible, false);

  _resetWindowStoreForTest();
});

test('fitVisibleWindowsToViewport reclamps an already-open window after viewport resize', () => {
  _resetWindowStoreForTest();
  showWindow('projects');

  fitVisibleWindowsToViewport({ width: 320, height: 700 });

  const projects = get(windowStore).projects;
  assert.deepEqual(
    {
      x: projects.x,
      y: projects.y,
      width: projects.width,
      height: projects.height,
    },
    { x: 0, y: 80, width: 320, height: 500 },
  );

  _resetWindowStoreForTest();
});

test('Given a sketch workspace When opened or restored Then its window remains available', () => {
  _resetWindowStoreForTest();

  showWindow('sketch');
  assert.equal(get(windowStore).sketch.visible, true);

  const restored = _mergeDbLayout({
    schemaVersion: 1,
    rememberLayout: true,
    windows: {
      params: { visible: true, minimized: false, x: 520, y: 80, width: 360, height: 480, z: 4 },
      sketch: { visible: true, minimized: false, x: 180, y: 120, width: 760, height: 520, z: 9 },
    },
  });
  assert.equal(restored.params.visible, true);
  assert.equal(restored.sketch.visible, true);

  _resetWindowStoreForTest();
});
