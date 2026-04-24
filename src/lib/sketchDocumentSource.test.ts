import assert from 'node:assert/strict';
import test from 'node:test';

import { buildSketchSuggestionDocument, buildSketchSuggestionRequest } from './sketchSuggestionDocument';
import {
  EMPTY_SKETCH_DOCUMENT_SOURCE_ERROR,
  formatSketchDocumentSource,
  sketchDocumentSummary,
} from './sketchDocumentSource';
import type { SketchStroke } from './sketchWorkspaceState';

const frontRectangle: SketchStroke = {
  primitiveId: 'primitive-front-7',
  view: 'front',
  points: [
    [10, 20],
    [40, 20],
    [40, 50],
    [10, 50],
    [10, 20],
  ],
  closed: true,
};

const topTriangle: SketchStroke = {
  primitiveId: 'primitive-top-3',
  view: 'top',
  points: [
    [5, 5],
    [25, 5],
    [15, 30],
    [5, 5],
  ],
  closed: true,
};

test('formatSketchDocumentSource returns pretty camelCase JSON for document payloads', () => {
  const document = buildSketchSuggestionDocument([frontRectangle]);

  assert.ok(document);
  const source = formatSketchDocumentSource(document);

  assert.match(source, /{\n  "documentId": "workspace-sketch-document"/);
  assert.match(source, /"activeSketchId": "sketch-front"/);
  assert.match(source, /"primitiveId": "primitive-front-7"/);
  assert.doesNotMatch(source, /document_id|active_sketch_id|primitive_id|constraint_id/);
});

test('formatSketchDocumentSource keeps SketchSuggestionRequest wrapper when supplied', () => {
  const request = buildSketchSuggestionRequest([frontRectangle]);

  assert.ok(!('error' in request));
  const source = formatSketchDocumentSource(request);

  assert.match(source, /{\n  "document": {\n    "documentId": "workspace-sketch-document"/);
  assert.match(source, /"primitiveId": "primitive-front-7"/);
});

test('sketchDocumentSummary returns stable line and primitive counts', () => {
  const document = buildSketchSuggestionDocument([frontRectangle, topTriangle]);

  assert.ok(document);
  const result = sketchDocumentSummary(document);

  assert.equal(result.summary, '90 lines / 2 sketches / 2 primitives');
  assert.equal(result.lineCount, 90);
  assert.equal(result.primitiveCount, 2);
  assert.deepEqual(result.rows, [
    { id: 'documentId', label: 'Document ID', value: 'workspace-sketch-document' },
    { id: 'activeSketchId', label: 'Active sketch', value: 'sketch-front' },
    { id: 'lineCount', label: 'JSON lines', value: '90 lines' },
    { id: 'primitiveCount', label: 'Primitives', value: '2 primitives' },
  ]);
});

test('sketchDocumentSummary reads primitive count from SketchSuggestionRequest document', () => {
  const request = buildSketchSuggestionRequest([frontRectangle]);

  assert.ok(!('error' in request));
  const result = sketchDocumentSummary(request);

  assert.equal(result.primitiveCount, 1);
  assert.equal(result.rows.find((row) => row.id === 'documentId')?.value, 'workspace-sketch-document');
});

test('null sketch document source returns empty source and error summary', () => {
  assert.equal(formatSketchDocumentSource(null), '');
  assert.deepEqual(sketchDocumentSummary(null), {
    summary: EMPTY_SKETCH_DOCUMENT_SOURCE_ERROR,
    rows: [],
    lineCount: 0,
    primitiveCount: 0,
    error: EMPTY_SKETCH_DOCUMENT_SOURCE_ERROR,
  });
});
