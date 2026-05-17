import assert from 'node:assert/strict';
import test from 'node:test';

import { selectThreadPreviewImage } from './projectPreview';

test('selectThreadPreviewImage prefers fresh viewport preview over stale latest cache', () => {
  assert.equal(
    selectThreadPreviewImage(
      { messages: [{ imageData: 'data:image/png;base64,old-message' }] },
      { id: 'latest-version', imageData: 'data:image/png;base64,stale-latest' },
      { messageId: 'latest-version', imageData: 'data:image/png;base64,fresh' },
    ),
    'data:image/png;base64,fresh',
  );
});

test('selectThreadPreviewImage rejects fresh preview from older same-thread version', () => {
  assert.equal(
    selectThreadPreviewImage(
      { messages: [{ imageData: 'data:image/png;base64,old-message' }] },
      { id: 'latest-version', imageData: null },
      { messageId: 'older-version', imageData: 'data:image/png;base64,stale' },
    ),
    null,
  );
});

test('selectThreadPreviewImage uses latest preview before message fallback', () => {
  assert.equal(
    selectThreadPreviewImage(
      { messages: [{ imageData: 'data:image/png;base64,old-message' }] },
      { id: 'latest-version', imageData: 'data:image/png;base64,latest' },
      undefined,
    ),
    'data:image/png;base64,latest',
  );
});

test('selectThreadPreviewImage treats explicit empty fresh preview as no preview', () => {
  assert.equal(
    selectThreadPreviewImage(
      { messages: [{ imageData: 'data:image/png;base64,old-message' }] },
      { id: 'latest-version', imageData: 'data:image/png;base64,latest' },
      { messageId: 'latest-version', imageData: '   ' },
    ),
    null,
  );
});
