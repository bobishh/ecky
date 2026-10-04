import assert from 'node:assert/strict';
import test from 'node:test';

import {
  captureWorkspaceReference,
  isWorkspaceCaptureEnabled,
  readWorkspaceCapturePrefs,
  setWorkspaceCaptureEnabled,
  workspaceCaptureScopeKey,
} from './workspaceCapture';

test('Given unavailable viewport capture When preparing a reference Then attachment staging never runs', async () => {
  let staged = false;
  await assert.rejects(captureWorkspaceReference(async () => null, async () => {
    staged = true;
    return { path: '', name: 'unexpected.png', type: 'image', explanation: '' };
  }), /Workspace image capture failed/);
  assert.equal(staged, false);
});

test('Given a renderer failure When preparing a reference Then its raw diagnostic reaches the caller', async () => {
  const failure = new Error('SecurityError: viewport canvas is tainted');
  await assert.rejects(captureWorkspaceReference(async () => { throw failure; }, async () => {
    throw new Error('staging must not run');
  }), (error) => error === failure);
});

test('workspaceCaptureScopeKey uses a stable fallback for new threads', () => {
  assert.equal(workspaceCaptureScopeKey('thread-1'), 'thread-1');
  assert.equal(workspaceCaptureScopeKey('  '), '__new__');
  assert.equal(workspaceCaptureScopeKey(null), '__new__');
});

test('setWorkspaceCaptureEnabled stores and removes per-thread flags', () => {
  const enabled = setWorkspaceCaptureEnabled({}, 'thread-1', true);
  assert.equal(enabled['thread-1'], true);
  assert.equal(isWorkspaceCaptureEnabled(enabled, 'thread-1'), true);

  const cleared = setWorkspaceCaptureEnabled(enabled, 'thread-1', false);
  assert.equal(isWorkspaceCaptureEnabled(cleared, 'thread-1'), false);
  assert.equal('thread-1' in cleared, false);
});

test('readWorkspaceCapturePrefs tolerates malformed storage payloads', () => {
  const badStorage = {
    getItem() {
      return '{';
    },
  };
  assert.deepEqual(readWorkspaceCapturePrefs(badStorage), {});
});
