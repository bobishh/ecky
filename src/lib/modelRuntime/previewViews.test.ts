import assert from 'node:assert/strict';
import test from 'node:test';

import { buildPreviewViewTransforms, mergePreviewTransforms, resolveActivePreviewView } from './previewViews';

test('resolveActivePreviewView falls back to first authored preview view', () => {
  const manifest = {
    previewViews: [
      { viewId: 'preview-exploded', label: 'exploded', offsets: [] },
      { viewId: 'preview-closed', label: 'closed', offsets: [] },
    ],
  } as any;

  assert.equal(resolveActivePreviewView(manifest, null)?.viewId, 'preview-exploded');
  assert.equal(resolveActivePreviewView(manifest, 'preview-closed')?.viewId, 'preview-closed');
  assert.equal(resolveActivePreviewView(manifest, 'missing')?.viewId, 'preview-exploded');
});

test('buildPreviewViewTransforms turns offsets into preview-only translations', () => {
  const manifest = {
    previewViews: [
      {
        viewId: 'preview-exploded',
        label: 'exploded',
        offsets: [{ partId: 'top-half', dx: 0, dy: 0, dz: 40 }],
      },
    ],
  } as any;

  assert.deepEqual(buildPreviewViewTransforms(manifest, 'preview-exploded'), {
    'top-half': {
      anchor: { x: 0, y: 0, z: 0 },
      scale: { x: 1, y: 1, z: 1 },
      translate: { x: 0, y: 0, z: 40 },
    },
  });
});

test('mergePreviewTransforms preserves imported scaling and overlays authored translation', () => {
  const base = {
    body: {
      anchor: { x: 12, y: 14, z: 0 },
      scale: { x: 1.2, y: 1, z: 1 },
    },
  };
  const overlay = {
    body: {
      anchor: { x: 0, y: 0, z: 0 },
      scale: { x: 1, y: 1, z: 1 },
      translate: { x: 0, y: 0, z: 18 },
    },
  };

  assert.deepEqual(mergePreviewTransforms(base, overlay), {
    body: {
      anchor: { x: 12, y: 14, z: 0 },
      scale: { x: 1.2, y: 1, z: 1 },
      translate: { x: 0, y: 0, z: 18 },
    },
  });
});
