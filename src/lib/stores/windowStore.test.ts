import assert from 'node:assert/strict';
import test from 'node:test';

import { get } from 'svelte/store';

import {
  _resetWindowStoreForTest,
  showWindow,
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
