import assert from 'node:assert/strict';
import test from 'node:test';

import { deriveViewerBusyState } from './viewerBusyState';

test('backend render lock activity shows the rendering preloader', () => {
  assert.deepEqual(deriveViewerBusyState({ geometryRenderActive: true, projectFolderRenderPending: false }), {
    showViewerBusyMask: true,
    viewerBusyPhase: 'rendering',
    viewerBusyText: 'Rendering geometry.',
  });
});

test('active project folder change shows the settling preloader before backend render lock', () => {
  assert.deepEqual(deriveViewerBusyState({ geometryRenderActive: false, projectFolderRenderPending: true }), {
    showViewerBusyMask: true,
    viewerBusyPhase: 'generating',
    viewerBusyText: 'Settling changed source.',
  });
});

test('viewport preloader stays absent without render activity', () => {
  assert.deepEqual(deriveViewerBusyState({ geometryRenderActive: false, projectFolderRenderPending: false }), {
    showViewerBusyMask: false,
    viewerBusyPhase: null,
    viewerBusyText: null,
  });
});

test('given the active version has a display mesh when topology runs then the viewport stays usable', () => {
  const busy = deriveViewerBusyState({
    geometryRenderActive: true,
    projectFolderRenderPending: true,
    hasEarlyDisplayMesh: true,
  });
  assert.equal(busy.showViewerBusyMask, false);
});
