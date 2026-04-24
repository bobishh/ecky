import assert from 'node:assert/strict';
import test from 'node:test';

import {
  buildSketchSuggestionDocument,
  buildSketchSuggestionRequest,
  WORKSPACE_SKETCH_DOCUMENT_ID,
} from './sketchSuggestionDocument';
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

test('buildSketchSuggestionRequest turns drawn front rectangle into camelCase SketchDocument', () => {
  const request = buildSketchSuggestionRequest([frontRectangle]);

  assert.ok(!('error' in request));
  assert.deepEqual(request, {
    document: {
      documentId: WORKSPACE_SKETCH_DOCUMENT_ID,
      sketches: [
        {
          sketchId: 'sketch-front',
          view: 'front',
          primitives: [
            {
              primitiveId: 'primitive-front-7',
              kind: 'polyline',
              points: frontRectangle.points,
              closed: true,
            },
          ],
          constraints: [
            {
              constraintId: 'primitive-front-7-closed',
              kind: 'closed',
              targetIds: ['primitive-front-7'],
            },
          ],
        },
      ],
      activeSketchId: 'sketch-front',
      units: 'mm',
      metadata: { source: 'workspace' },
    },
  });

  const serialized = JSON.stringify(request);
  assert.match(serialized, /"documentId"/);
  assert.match(serialized, /"activeSketchId"/);
  assert.match(serialized, /"primitiveId"/);
  assert.match(serialized, /"constraintId"/);
  assert.doesNotMatch(serialized, /document_id|active_sketch_id|primitive_id|constraint_id/);
});

test('buildSketchSuggestionRequest keeps drawn primitiveId instead of seed id', () => {
  const request = buildSketchSuggestionRequest([frontRectangle]);

  assert.ok(!('error' in request));
  const primitive = request.document.sketches?.[0]?.primitives?.[0];
  assert.equal(primitive?.primitiveId, 'primitive-front-7');
  assert.notEqual(primitive?.primitiveId, 'seed');
});

test('buildSketchSuggestionRequest adds width and height dimension constraints for locked profile dimensions', () => {
  const request = buildSketchSuggestionRequest([
    {
      ...frontRectangle,
      dimensionLocks: { width: true, height: true },
    },
  ]);

  assert.ok(!('error' in request));
  assert.deepEqual(request.document.sketches?.[0]?.constraints, [
    {
      constraintId: 'primitive-front-7-closed',
      kind: 'closed',
      targetIds: ['primitive-front-7'],
    },
    {
      constraintId: 'primitive-front-7-width-dimension',
      kind: 'dimension',
      targetIds: ['primitive-front-7'],
      value: 30,
    },
    {
      constraintId: 'primitive-front-7-height-dimension',
      kind: 'dimension',
      targetIds: ['primitive-front-7'],
      value: 30,
    },
  ]);
});

test('buildSketchSuggestionRequest ignores open strokes without warnings', () => {
  const request = buildSketchSuggestionRequest([
    {
      primitiveId: 'primitive-front-open',
      view: 'front',
      points: [
        [0, 0],
        [20, 20],
      ],
      closed: false,
    },
    frontRectangle,
  ]);

  assert.ok(!('error' in request));
  assert.equal(request.document.sketches?.length, 1);
  assert.deepEqual(
    request.document.sketches?.flatMap((sketch) => sketch.primitives?.map((primitive) => primitive.primitiveId) ?? []),
    ['primitive-front-7'],
  );
  assert.ok(!('warnings' in request));
});

test('buildSketchSuggestionDocument returns null and request returns error for no closed profiles', () => {
  const strokes: SketchStroke[] = [
    {
      primitiveId: 'primitive-front-open',
      view: 'front',
      points: [
        [0, 0],
        [20, 20],
      ],
      closed: false,
    },
  ];

  assert.equal(buildSketchSuggestionDocument(strokes), null);
  assert.deepEqual(buildSketchSuggestionRequest(strokes), { error: 'Close profile before suggestions.' });
});
