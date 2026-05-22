import assert from 'node:assert/strict';
import test from 'node:test';

import { get } from 'svelte/store';

import {
  ALL_WINDOW_IDS,
  _resetWindowStoreForTest,
  bringToFront,
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
