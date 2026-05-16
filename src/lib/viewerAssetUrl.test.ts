import test from 'node:test';
import assert from 'node:assert/strict';

import { resolveViewerAssetUrl } from './viewerAssetUrl';

test('resolveViewerAssetUrl preserves immutable generated artifact URLs', () => {
  assert.equal(resolveViewerAssetUrl('/mock/cache/preview.stl'), '/mock/cache/preview.stl');
  assert.equal(
    resolveViewerAssetUrl('asset://localhost/mock/cache/preview.stl?hash=abc'),
    'asset://localhost/mock/cache/preview.stl?hash=abc',
  );
});

test('resolveViewerAssetUrl does not append timestamp cache busting', () => {
  const resolvedUrl = resolveViewerAssetUrl('/mock/cache/preview.stl', 'model:hash');

  assert.equal(resolvedUrl.includes('t='), false);
});

test('resolveViewerAssetUrl appends deterministic model cache key when provided', () => {
  assert.equal(
    resolveViewerAssetUrl('/mock/cache/preview.stl', 'thread:msg:model:hash'),
    '/mock/cache/preview.stl?v=thread%3Amsg%3Amodel%3Ahash',
  );
  assert.equal(
    resolveViewerAssetUrl('/mock/cache/preview.stl?asset=true', 'model hash'),
    '/mock/cache/preview.stl?asset=true&v=model%20hash',
  );
});
