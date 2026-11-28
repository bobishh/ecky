import test from 'node:test';
import assert from 'node:assert/strict';
import { get } from 'svelte/store';
import {
  createParameterDraftStore,
  effectiveDraftParams,
  hasActiveParameterDraft,
} from './parameterDraft';

test('Given a version base When panel and viewport both edit parameters Then both merge into one shared draft without wiping each other', () => {
  const draft = createParameterDraftStore();
  draft.syncBase('t1:m1:s1', { radius: 5, height: 10 });

  // Panel edits height
  draft.setDraft({ radius: 5, height: 14 });
  assert.equal(hasActiveParameterDraft(get(draft)), true);
  assert.deepEqual(effectiveDraftParams(get(draft)), { radius: 5, height: 14 });

  // Viewport overlay patches radius onto the same draft
  draft.patchDraft({ radius: 8 });
  assert.deepEqual(effectiveDraftParams(get(draft)), { radius: 8, height: 14 });
  assert.deepEqual(get(draft).baseParams, { radius: 5, height: 10 });

  // Cancel restores base parameters cleanly
  draft.clearDraft();
  assert.equal(hasActiveParameterDraft(get(draft)), false);
  assert.deepEqual(effectiveDraftParams(get(draft)), { radius: 5, height: 10 });
});

test('Given an active draft When returning to saved base values Then draft clears automatically', () => {
  const draft = createParameterDraftStore();
  draft.syncBase('t1:m1:s1', { radius: 5 });
  draft.patchDraft({ radius: 7 });
  assert.equal(hasActiveParameterDraft(get(draft)), true);

  draft.patchDraft({ radius: 5 });
  assert.equal(hasActiveParameterDraft(get(draft)), false);
  assert.equal(get(draft).draftParams, null);
});

test('Given an active draft When version changes Then draft resets to the new version base', () => {
  const draft = createParameterDraftStore();
  draft.syncBase('t1:m1:s1', { radius: 5 });
  draft.patchDraft({ radius: 9 });
  assert.equal(hasActiveParameterDraft(get(draft)), true);

  draft.syncBase('t1:m2:s2', { radius: 9 });
  assert.equal(hasActiveParameterDraft(get(draft)), false);
  assert.deepEqual(effectiveDraftParams(get(draft)), { radius: 9 });
});
