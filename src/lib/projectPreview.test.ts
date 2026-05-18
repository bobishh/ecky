import assert from 'node:assert/strict';
import test from 'node:test';

import { selectThreadPreviewImage } from './projectPreview';

const renderableMessage = (imageData: string) => ({
  role: 'assistant' as const,
  status: 'success' as const,
  artifactBundle: { previewStlPath: '/tmp/model.stl' },
  imageData,
});

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

test('selectThreadPreviewImage uses older fresh preview when latest lacks preview', () => {
  assert.equal(
    selectThreadPreviewImage(
      { messages: [{ imageData: 'data:image/png;base64,old-message' }] },
      { id: 'latest-version', imageData: null },
      { messageId: 'older-version', imageData: 'data:image/png;base64,stale' },
    ),
    'data:image/png;base64,stale',
  );
});

test('selectThreadPreviewImage rejects older fresh preview when latest has preview', () => {
  assert.equal(
    selectThreadPreviewImage(
      { messages: [{ imageData: 'data:image/png;base64,old-message' }] },
      { id: 'latest-version', imageData: 'data:image/png;base64,latest' },
      { messageId: 'older-version', imageData: 'data:image/png;base64,stale' },
    ),
    'data:image/png;base64,latest',
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

test('selectThreadPreviewImage falls back when latest fresh preview is empty', () => {
  assert.equal(
    selectThreadPreviewImage(
      { messages: [{ imageData: 'data:image/png;base64,old-message' }] },
      { id: 'latest-version', imageData: 'data:image/png;base64,latest' },
      { messageId: 'latest-version', imageData: '   ' },
    ),
    'data:image/png;base64,latest',
  );
});

test('selectThreadPreviewImage falls back only to renderable assistant version images', () => {
  assert.equal(
    selectThreadPreviewImage(
      {
        messages: [
          { role: 'user', status: 'success', artifactBundle: null, imageData: 'data:image/png;base64,user' },
          { role: 'assistant', status: 'success', artifactBundle: null, imageData: 'data:image/png;base64,concept' },
          renderableMessage('data:image/png;base64,version'),
        ],
      },
      undefined,
      undefined,
    ),
    'data:image/png;base64,version',
  );
});

test('selectThreadPreviewImage ignores non-renderable message image fallback', () => {
  assert.equal(
    selectThreadPreviewImage(
      {
        messages: [
          { role: 'user', status: 'success', artifactBundle: null, imageData: 'data:image/png;base64,user' },
          { role: 'assistant', status: 'success', artifactBundle: null, imageData: 'data:image/png;base64,concept' },
        ],
      },
      undefined,
      undefined,
    ),
    null,
  );
});
