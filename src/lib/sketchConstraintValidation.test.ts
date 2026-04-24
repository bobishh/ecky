import assert from 'node:assert/strict';
import test from 'node:test';

import { repairSketchDocumentDimensionConstraints, validateSketchDocumentConstraints } from './sketchConstraintValidation';
import type { SketchDocument } from './tauri/contracts';

function documentWithConstraints(constraints: NonNullable<NonNullable<SketchDocument['sketches']>[number]['constraints']>): SketchDocument {
  return {
    documentId: 'doc-test',
    units: 'mm',
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
              [60, 20],
              [60, 45],
              [10, 45],
              [10, 20],
            ],
            closed: true,
          },
        ],
        constraints,
      },
    ],
  };
}

test('passes dimension constraints measured from primitive bounding box logical points', () => {
  const result = validateSketchDocumentConstraints(documentWithConstraints([
    {
      constraintId: 'primitive-front-1-width-dimension',
      kind: 'dimension',
      targetIds: ['primitive-front-1'],
      value: 50,
    },
    {
      constraintId: 'primitive-front-1-height-dimension',
      kind: 'dimension',
      targetIds: ['primitive-front-1'],
      value: 25,
    },
  ]));

  assert.deepEqual(result, {
    passed: true,
    evidence: [
      "sketch 'sketch-front' primitive 'primitive-front-1' width dimension matched 50mm.",
      "sketch 'sketch-front' primitive 'primitive-front-1' height dimension matched 25mm.",
    ],
  });
});

test('passes with no dimension constraints', () => {
  const result = validateSketchDocumentConstraints(documentWithConstraints([
    {
      constraintId: 'primitive-front-1-closed',
      kind: 'closed',
      targetIds: ['primitive-front-1'],
    },
  ]));

  assert.deepEqual(result, { passed: true, evidence: ['No dimension constraints.'] });
});

test('fails mismatched dimensions with deterministic measured issue', () => {
  const result = validateSketchDocumentConstraints(documentWithConstraints([
    {
      constraintId: 'primitive-front-1-width-dimension',
      kind: 'dimension',
      targetIds: ['primitive-front-1'],
      value: 99,
    },
  ]));

  assert.deepEqual(result, {
    passed: false,
    issues: ["sketch 'sketch-front' primitive 'primitive-front-1' width dimension expected 99mm but measured 50mm."],
  });
});

test('fails missing value missing target invalid points and unknown dimension kind', () => {
  const document = documentWithConstraints([
    {
      constraintId: 'primitive-front-1-width-dimension',
      kind: 'dimension',
      targetIds: ['primitive-front-1'],
      value: null,
    },
    {
      constraintId: 'primitive-front-1-height-dimension',
      kind: 'dimension',
      targetIds: ['primitive-front-missing'],
      value: 25,
    },
    {
      constraintId: 'primitive-front-bad-width-dimension',
      kind: 'dimension',
      targetIds: ['primitive-front-bad'],
      value: 10,
    },
    {
      constraintId: 'primitive-front-1-depth-dimension',
      kind: 'dimension',
      targetIds: ['primitive-front-1'],
      value: 12,
    },
  ]);
  document.sketches?.[0]?.primitives?.push({
    primitiveId: 'primitive-front-bad',
    kind: 'polyline',
    points: [],
  });

  assert.deepEqual(validateSketchDocumentConstraints(document), {
    passed: false,
    issues: [
      "sketch 'sketch-front' dimension constraint 'primitive-front-1-width-dimension' has missing or non-finite value.",
      "sketch 'sketch-front' dimension constraint 'primitive-front-1-height-dimension' targets missing primitive 'primitive-front-missing'.",
      "sketch 'sketch-front' primitive 'primitive-front-bad' has invalid or no points.",
      "sketch 'sketch-front' dimension constraint 'primitive-front-1-depth-dimension' is neither width nor height.",
    ],
  });
});

test('repairs stale dimension constraints to measured primitive bounds', () => {
  const sourceDocument = documentWithConstraints([
    {
      constraintId: 'primitive-front-1-width-dimension',
      kind: 'dimension',
      targetIds: ['primitive-front-1'],
      value: 99,
    },
    {
      constraintId: 'primitive-front-1-height-dimension',
      kind: 'dimension',
      targetIds: ['primitive-front-1'],
      value: 25,
    },
  ]);

  const result = repairSketchDocumentDimensionConstraints(sourceDocument);

  assert.ok(!('error' in result));
  assert.equal(sourceDocument.sketches?.[0]?.constraints?.[0]?.value, 99);
  assert.equal(result.document.sketches?.[0]?.constraints?.[0]?.value, 50);
  assert.equal(result.document.sketches?.[0]?.constraints?.[1]?.value, 25);
  assert.deepEqual(result.evidence, [
    "sketch 'sketch-front' primitive 'primitive-front-1' width dimension repaired 99mm -> 50mm.",
  ]);
});

test('repair reports no repairable dimension mismatch for matching constraints', () => {
  assert.deepEqual(
    repairSketchDocumentDimensionConstraints(documentWithConstraints([
      {
        constraintId: 'primitive-front-1-width-dimension',
        kind: 'dimension',
        targetIds: ['primitive-front-1'],
        value: 50,
      },
    ])),
    { error: 'No repairable dimension constraint mismatch.' },
  );
});

test('repair returns raw structural issue for unsupported dimension constraint', () => {
  assert.deepEqual(
    repairSketchDocumentDimensionConstraints(documentWithConstraints([
      {
        constraintId: 'primitive-front-1-depth-dimension',
        kind: 'dimension',
        targetIds: ['primitive-front-1'],
        value: 12,
      },
    ])),
    {
      error: "sketch 'sketch-front' dimension constraint 'primitive-front-1-depth-dimension' is neither width nor height.",
    },
  );
});
