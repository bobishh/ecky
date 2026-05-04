import assert from 'node:assert/strict';
import test from 'node:test';

import { buildSketchDocumentFromBrepProjection } from './sketchBrepDerivedSketch';
import type { BrepHiddenLineProjectionResponse } from './tauri/contracts';
import type { SketchBrepDerivedSketchResult } from './sketchBrepDerivedSketch';

const projection: BrepHiddenLineProjectionResponse = {
  modelId: 'accepted-model',
  sourceArtifactPath: '/tmp/source-model.step',
  views: [
    {
      view: 'front',
      direction: [0, -1, 0],
      visibleEdges: [
        { edgeId: 'front-a', points: [[10, 20], [60, 20]], sourceClass: 'V' },
        { edgeId: 'front-b', points: [[60, 20], [60, 50]], sourceClass: 'V' },
      ],
      hiddenEdges: [{ edgeId: 'front-c', points: [[10, 50], [60, 50]], sourceClass: 'H' }],
    },
    {
      view: 'top',
      direction: [0, 0, -1],
      visibleEdges: [{ edgeId: 'top-a', points: [[5, 8], [25, 18]], sourceClass: 'V' }],
      hiddenEdges: [],
    },
    {
      view: 'side',
      direction: [-1, 0, 0],
      visibleEdges: [{ edgeId: 'side-a', points: [[2, 4], [12, 16]], sourceClass: 'V' }],
      hiddenEdges: [],
    },
  ],
  warnings: [],
  validation: { passed: true, issues: [], evidence: [] },
};

test('buildSketchDocumentFromBrepProjection creates derived closed polyline sketches with provenance', () => {
  const result = buildSketchDocumentFromBrepProjection(projection);
  const derived = assertDerivedResult(result);

  assert.equal(derived.document.documentId, 'derived-brep-accepted-model');
  assert.equal(derived.document.metadata?.provenance, 'derivedFromBRep');
  assert.equal(derived.document.metadata?.sourceArtifactPath, '/tmp/source-model.step');
  assert.equal(derived.document.metadata?.sourceModelId, 'accepted-model');
  assert.deepEqual(derived.document.sketches?.map((sketch) => sketch.view), ['front', 'top', 'side']);
  assert.deepEqual(derived.document.sketches?.[0]?.primitives?.[0]?.points, [
    [10, 20],
    [60, 20],
    [60, 50],
    [10, 50],
    [10, 20],
  ]);
  assert.equal(derived.document.sketches?.[0]?.primitives?.[0]?.primitiveId, 'derived-brep-front');
  assert.match(derived.evidence, /DERIVED FROM BREP \/ NOT AUTHORING HISTORY/i);
});

test('buildSketchDocumentFromBrepProjection skips views with no usable edge points', () => {
  const result = buildSketchDocumentFromBrepProjection({
    ...projection,
    views: [{ view: 'front', direction: [0, -1, 0], visibleEdges: [], hiddenEdges: [] }, projection.views?.[1] ?? failMissingTopView()],
  });
  const derived = assertDerivedResult(result);

  assert.deepEqual(derived.document.sketches?.map((sketch) => sketch.view), ['top']);
  assert.equal(derived.document.activeSketchId, 'derived-brep-top');
});

test('buildSketchDocumentFromBrepProjection preserves a closed projection edge loop before falling back to bounds', () => {
  const result = buildSketchDocumentFromBrepProjection({
    ...projection,
    views: [
      {
        view: 'front',
        direction: [0, -1, 0],
        visibleEdges: [
          { edgeId: 'front-a', points: [[0, 0], [40, 0]], sourceClass: 'V' },
          { edgeId: 'front-b', points: [[40, 0], [55, 20]], sourceClass: 'V' },
          { edgeId: 'front-c', points: [[55, 20], [20, 35]], sourceClass: 'V' },
          { edgeId: 'front-d', points: [[20, 35], [0, 0]], sourceClass: 'V' },
        ],
        hiddenEdges: [],
      },
    ],
  });
  const derived = assertDerivedResult(result);

  assert.deepEqual(derived.document.sketches?.[0]?.primitives?.[0]?.points, [
    [0, 0],
    [40, 0],
    [55, 20],
    [20, 35],
    [0, 0],
  ]);
});

test('buildSketchDocumentFromBrepProjection preserves multiple closed projection loops as separate primitives', () => {
  const result = buildSketchDocumentFromBrepProjection({
    ...projection,
    views: [
      {
        view: 'front',
        direction: [0, -1, 0],
        visibleEdges: [
          { edgeId: 'outer-a', points: [[0, 0], [80, 0]], sourceClass: 'V' },
          { edgeId: 'outer-b', points: [[80, 0], [80, 50]], sourceClass: 'V' },
          { edgeId: 'outer-c', points: [[80, 50], [0, 50]], sourceClass: 'V' },
          { edgeId: 'outer-d', points: [[0, 50], [0, 0]], sourceClass: 'V' },
          { edgeId: 'inner-a', points: [[25, 18], [45, 18]], sourceClass: 'V' },
          { edgeId: 'inner-b', points: [[45, 18], [45, 34]], sourceClass: 'V' },
          { edgeId: 'inner-c', points: [[45, 34], [25, 34]], sourceClass: 'V' },
          { edgeId: 'inner-d', points: [[25, 34], [25, 18]], sourceClass: 'V' },
        ],
        hiddenEdges: [],
      },
    ],
  });
  const derived = assertDerivedResult(result);

  assert.equal(derived.document.sketches?.[0]?.primitives?.length, 2);
  assert.equal(derived.document.sketches?.[0]?.primitives?.[0]?.primitiveId, 'derived-brep-front');
  assert.equal(derived.document.sketches?.[0]?.primitives?.[1]?.primitiveId, 'derived-brep-front-2');
  assert.deepEqual(derived.document.sketches?.[0]?.primitives?.[1]?.points, [
    [25, 18],
    [45, 18],
    [45, 34],
    [25, 34],
    [25, 18],
  ]);
});

test('buildSketchDocumentFromBrepProjection errors when projection has no drawable edges', () => {
  const result = buildSketchDocumentFromBrepProjection({
    ...projection,
    views: [{ view: 'front', direction: [0, -1, 0], visibleEdges: [], hiddenEdges: [] }],
  });

  assert.deepEqual(result, { error: 'BRep projection has no drawable sketch bounds.' });
});

function assertDerivedResult(
  result: SketchBrepDerivedSketchResult,
): Extract<SketchBrepDerivedSketchResult, { document: unknown }> {
  if ('error' in result) assert.fail(result.error);
  return result;
}

function failMissingTopView(): never {
  assert.fail('test projection top view missing');
}
