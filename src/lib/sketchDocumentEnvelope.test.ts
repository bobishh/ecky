import assert from 'node:assert/strict';
import test from 'node:test';

import { parseSketchDocumentEnvelope } from './sketchDocumentEnvelope';
import type { SketchDocument } from './tauri/contracts';

const document: SketchDocument = {
  documentId: 'doc-envelope',
  sketches: [
    {
      sketchId: 'sketch-front',
      view: 'front',
      primitives: [
        {
          primitiveId: 'primitive-front-1',
          kind: 'polyline',
          points: [
            [10, 20],
            [40, 20],
            [40, 50],
            [10, 50],
            [10, 20],
          ],
          closed: true,
        },
      ],
    },
  ],
  activeSketchId: 'sketch-front',
  units: 'mm',
};

function encodeUtf8Base64(input: string): string {
  return Buffer.from(input, 'utf8').toString('base64');
}

test('parseSketchDocumentEnvelope returns embedded SketchDocument from ecky source comment', () => {
  const source = [
    'module sketch',
    `; ecky-sketch-document-base64: ${encodeUtf8Base64(JSON.stringify(document))}`,
    'end',
  ].join('\n');

  assert.deepEqual(parseSketchDocumentEnvelope(source), { document });
});

test('parseSketchDocumentEnvelope rejects missing marker', () => {
  assert.deepEqual(parseSketchDocumentEnvelope('module sketch\nend'), {
    error: 'Sketch document marker missing.',
  });
});

test('parseSketchDocumentEnvelope rejects invalid base64', () => {
  assert.deepEqual(parseSketchDocumentEnvelope('; ecky-sketch-document-base64: not-base64'), {
    error: 'Sketch document base64 is invalid.',
  });
});

test('parseSketchDocumentEnvelope rejects invalid JSON', () => {
  const source = `; ecky-sketch-document-base64: ${encodeUtf8Base64('not json')}`;

  assert.deepEqual(parseSketchDocumentEnvelope(source), {
    error: 'Sketch document JSON is invalid.',
  });
});

test('parseSketchDocumentEnvelope rejects missing sketches', () => {
  const source = `; ecky-sketch-document-base64: ${encodeUtf8Base64(
    JSON.stringify({
      documentId: 'doc-empty',
      sketches: [],
      activeSketchId: null,
      units: 'mm',
    }),
  )}`;

  assert.deepEqual(parseSketchDocumentEnvelope(source), {
    error: 'Sketch document has no sketches.',
  });
});
