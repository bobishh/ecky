import assert from 'node:assert/strict';
import test from 'node:test';

import type { ViewportCameraState } from '../types/domain';
import {
  chooseViewportCaptureMode,
  rememberTargetCameraState,
  rememberTargetScreenshot,
  resolveFallbackScreenshotSource,
  viewportTargetKey,
} from './screenshot';

const sampleCamera: ViewportCameraState = {
  position: [140, 120, 140],
  target: [0, 24, 0],
  zoom: null,
  fov: 45,
};

test('chooseViewportCaptureMode uses the visible viewer only for matching target without override', () => {
  assert.equal(
    chooseViewportCaptureMode({
      currentView: 'workbench',
      currentThreadId: 'thread-1',
      currentMessageId: 'message-1',
      requestedThreadId: 'thread-1',
      requestedMessageId: 'message-1',
      cameraOverride: null,
      hasVisibleViewer: true,
    }),
    'visible-live',
  );

  assert.equal(
    chooseViewportCaptureMode({
      currentView: 'workbench',
      currentThreadId: 'thread-1',
      currentMessageId: 'message-1',
      requestedThreadId: 'thread-1',
      requestedMessageId: 'message-1',
      cameraOverride: sampleCamera,
      hasVisibleViewer: true,
    }),
    'hidden-target',
  );
});

test('chooseViewportCaptureMode requires user choice when the requested target is not the visible viewport', () => {
  assert.equal(
    chooseViewportCaptureMode({
      currentView: 'workbench',
      currentThreadId: 'thread-1',
      currentMessageId: 'message-1',
      requestedThreadId: 'thread-2',
      requestedMessageId: 'message-4',
      cameraOverride: null,
      hasVisibleViewer: true,
    }),
    'needs-user-choice',
  );

  assert.equal(
    chooseViewportCaptureMode({
      currentView: 'config',
      currentThreadId: 'thread-1',
      currentMessageId: 'message-1',
      requestedThreadId: 'thread-1',
      requestedMessageId: 'message-1',
      cameraOverride: null,
      hasVisibleViewer: false,
    }),
    'needs-user-choice',
  );
});

test('resolveFallbackScreenshotSource prefers the last live screenshot before hidden preview', () => {
  const key = viewportTargetKey('thread-1', 'message-1');
  const cached = rememberTargetScreenshot({}, key, {
    dataUrl: 'data:image/jpeg;base64,abc',
    width: 800,
    height: 600,
    camera: sampleCamera,
    capturedAt: 123,
  });

  assert.deepEqual(resolveFallbackScreenshotSource(cached, key), {
    kind: 'cached-live',
    capture: cached[key],
  });
  assert.deepEqual(resolveFallbackScreenshotSource({}, key), { kind: 'hidden-preview' });
});

test('rememberTargetCameraState persists per target and does not leak to a different target', () => {
  const firstKey = viewportTargetKey('thread-1', 'message-1');
  const secondKey = viewportTargetKey('thread-1', 'message-2');

  const cache = rememberTargetCameraState({}, firstKey, sampleCamera, true);
  assert.equal(cache[firstKey]?.target[1], 24);
  assert.equal(cache[secondKey], undefined);
});

test('rememberTargetCameraState ignores non-persistent override captures', () => {
  const key = viewportTargetKey('thread-1', 'message-1');
  const cache = rememberTargetCameraState({}, key, sampleCamera, false);
  assert.deepEqual(cache, {});
});
