import assert from 'node:assert/strict';
import test from 'node:test';

import { repairSketchDocumentEndpointGaps } from './sketchEndpointRepair';
import type { SketchDocument } from './tauri/contracts';

function documentWithPolyline(points: [number, number][], closed: boolean): SketchDocument {
  return {
    documentId: 'doc-gap',
    activeSketchId: 'sketch-front',
    units: 'mm',
    sketches: [
      {
        sketchId: 'sketch-front',
        view: 'front',
        primitives: [
          {
            primitiveId: 'primitive-front-gap',
            kind: 'polyline',
            points,
            closed,
          },
        ],
      },
    ],
  };
}

test('repairSketchDocumentEndpointGaps closes tiny open endpoint gap', () => {
  const result = repairSketchDocumentEndpointGaps(
    documentWithPolyline([
      [10, 20],
      [60, 20],
      [60, 50],
      [10, 50],
      [10.35, 20.25],
    ], false),
  );

  const primitive = result.document.sketches?.[0]?.primitives?.[0];
  assert.equal(primitive?.closed, true);
  assert.deepEqual(primitive?.points?.at(-1), [10, 20]);
  assert.deepEqual(result.evidence, [
    {
      primitiveId: 'primitive-front-gap',
      detail: "sketch 'sketch-front' primitive 'primitive-front-gap' closed endpoint gap 0.4301mm.",
    },
  ]);
});

test('repairSketchDocumentEndpointGaps leaves large open gap untouched', () => {
  const source = documentWithPolyline([
    [10, 20],
    [60, 20],
    [60, 50],
    [10, 50],
    [18, 28],
  ], false);

  const result = repairSketchDocumentEndpointGaps(source);
  const primitive = result.document.sketches?.[0]?.primitives?.[0];
  assert.equal(primitive?.closed, false);
  assert.deepEqual(primitive?.points?.at(-1), [18, 28]);
  assert.deepEqual(result.evidence, []);
});
