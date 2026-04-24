import assert from 'node:assert/strict';
import test from 'node:test';

import { buildDraftRequestFromSuggestion } from './sketchSuggestionAccept';
import type { SketchDocument, SketchFeatureSuggestion } from './tauri/contracts';

const document: SketchDocument = {
  documentId: 'doc-accept',
  sketches: [
    {
      sketchId: 'sketch-front',
      view: 'front',
      primitives: [
        {
          primitiveId: 'outer',
          kind: 'polyline',
          points: [
            [0, 0],
            [30, 0],
            [30, 12],
            [0, 12],
          ],
          closed: true,
        },
        {
          primitiveId: 'inner',
          kind: 'circle',
          points: [[10, 6]],
          radius: 3,
          closed: true,
        },
      ],
      constraints: [
        {
          constraintId: 'outer-closed',
          kind: 'closed',
          targetIds: ['outer'],
        },
        {
          constraintId: 'inner-closed',
          kind: 'closed',
          targetIds: ['inner'],
        },
      ],
    },
  ],
  activeSketchId: 'sketch-front',
  units: 'mm',
};

function suggestion(overrides: Partial<SketchFeatureSuggestion> = {}): SketchFeatureSuggestion {
  return {
    suggestionId: 'sketch-front:outer:extrude',
    sketchId: 'sketch-front',
    primitiveId: 'outer',
    partId: 'accepted-part',
    operation: 'extrude',
    amount: 12,
    symmetric: true,
    confidence: 0.95,
    reason: 'closed profile',
    ...overrides,
  };
}

test('buildDraftRequestFromSuggestion builds request from valid extrude suggestion', () => {
  const request = buildDraftRequestFromSuggestion(document, suggestion());

  assert.ok(!('error' in request));
  assert.deepEqual(request, {
    partId: 'accepted-part',
    sketch: {
      sketchId: 'sketch-front',
      view: 'front',
      primitives: [
        {
          primitiveId: 'outer',
          kind: 'polyline',
          points: [
            [0, 0],
            [30, 0],
            [30, 12],
            [0, 12],
          ],
          closed: true,
        },
      ],
      constraints: [
        {
          constraintId: 'outer-closed',
          kind: 'closed',
          targetIds: ['outer'],
        },
      ],
    },
    operation: 'extrude',
    amount: 12,
    symmetric: true,
  });
});

test('buildDraftRequestFromSuggestion returns error for missing sketch', () => {
  const request = buildDraftRequestFromSuggestion(document, suggestion({ sketchId: 'missing-sketch' }));

  assert.deepEqual(request, {
    error: "suggestion 'sketch-front:outer:extrude' references missing sketch 'missing-sketch'.",
  });
});

test('buildDraftRequestFromSuggestion returns error for missing primitive', () => {
  const request = buildDraftRequestFromSuggestion(document, suggestion({ primitiveId: 'missing-primitive' }));

  assert.deepEqual(request, {
    error: "suggestion 'sketch-front:outer:extrude' references missing primitive 'missing-primitive'.",
  });
});

test('buildDraftRequestFromSuggestion uses whole sketch when primitiveId is null', () => {
  const request = buildDraftRequestFromSuggestion(document, suggestion({ primitiveId: null }));

  assert.ok(!('error' in request));
  assert.deepEqual(
    request.sketch.primitives?.map((primitive) => primitive.primitiveId),
    ['outer', 'inner'],
  );
});
