import assert from 'node:assert/strict';
import test from 'node:test';

import {
  isWorkspaceCaptureEnabled,
  readWorkspaceCapturePrefs,
  setWorkspaceCaptureEnabled,
  workspaceCaptureScopeKey,
} from './workspaceCapture';

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
