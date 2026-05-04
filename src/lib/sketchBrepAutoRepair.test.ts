import assert from 'node:assert/strict';
import test from 'node:test';

import { autoRepairSketchDocumentFromBrepProjection } from './sketchBrepAutoRepair';
import type { BrepHiddenLineProjectionResponse, SketchDocument } from './tauri/contracts';

const document: SketchDocument = {
  documentId: 'doc-brep-repair',
  activeSketchId: 'sketch-front',
  units: 'mm',
  sketches: [
    {
      sketchId: 'sketch-front',
      view: 'front',
      primitives: [
        {
          primitiveId: 'primitive-front',
          kind: 'polyline',
          points: [
            [10, 20],
            [60, 20],
            [60, 50],
            [10, 50],
            [10, 20],
          ],
          closed: true,
        },
      ],
    },
  ],
};

function projection(
  issuePrimitiveId: string | null = 'primitive-front',
): BrepHiddenLineProjectionResponse {
  return {
    modelId: 'model-1',
    sourceArtifactPath: '/tmp/model.step',
    views: [
      {
        view: 'front',
        direction: [0, -1, 0],
        visibleEdges: [
          { edgeId: 'front-bottom', points: [[0, 0], [80, 0]], sourceClass: 'V' },
          { edgeId: 'front-right', points: [[80, 0], [80, 40]], sourceClass: 'V' },
        ],
        hiddenEdges: [
          { edgeId: 'front-top', points: [[0, 40], [80, 40]], sourceClass: 'H' },
        ],
      },
    ],
    warnings: [],
    validation: {
      passed: false,
      issues: [
        {
          sketchId: 'sketch-front',
          primitiveId: issuePrimitiveId,
          severity: 'error',
          message: 'Front bounds mismatch: sketch minX=10 maxX=60 minY=20 maxY=50, brep minX=0 maxX=80 minY=0 maxY=40.',
        },
      ],
      evidence: [],
    },
  };
}

function containmentProjection(): BrepHiddenLineProjectionResponse {
  return {
    ...projection(),
    views: [
      {
        view: 'front',
        direction: [0, -1, 0],
        visibleEdges: [
          { edgeId: 'front-bottom', points: [[10, 20], [64.2, 20]], sourceClass: 'V' },
          { edgeId: 'front-right', points: [[64.2, 20], [64.2, 50]], sourceClass: 'V' },
        ],
        hiddenEdges: [
          { edgeId: 'front-top', points: [[10, 50], [64.2, 50]], sourceClass: 'H' },
        ],
      },
    ],
    validation: {
      passed: false,
      issues: [
        {
          sketchId: 'sketch-front',
          primitiveId: 'primitive-front',
          severity: 'error',
          message: 'raw BREP/SKETCH containment mismatch: front edge front-v1 has 8 samples outside source profile, maxOutside=4.2mm',
        },
      ],
      evidence: [],
    },
  };
}

function hugeContainmentProjection(): BrepHiddenLineProjectionResponse {
  const huge = containmentProjection();
  huge.views = [
    {
      view: 'front',
      direction: [0, -1, 0],
      visibleEdges: [
        { edgeId: 'front-bottom', points: [[10, 20], [140, 20]], sourceClass: 'V' },
        { edgeId: 'front-right', points: [[140, 20], [140, 50]], sourceClass: 'V' },
      ],
      hiddenEdges: [
        { edgeId: 'front-top', points: [[10, 50], [140, 50]], sourceClass: 'H' },
      ],
    },
  ];
  return huge;
}

function firstPrimitivePoints(source: SketchDocument): [number, number][] {
  return source.sketches?.[0]?.primitives?.[0]?.points ?? [];
}

test('autoRepairSketchDocumentFromBrepProjection scales and translates closed polyline to BRep bounds', () => {
  const result = autoRepairSketchDocumentFromBrepProjection(document, projection());

  assert.equal(result.repaired, true);
  assert.equal(result.evidence[0]?.primitiveId, 'primitive-front');
  assert.equal(result.evidence[0]?.view, 'front');
  assert.match(result.evidence[0]?.detail ?? '', /BREP AUTO SNAP FRONT primitive-front bounds 50x30 -> 80x40/);
  assert.deepEqual(firstPrimitivePoints(result.document), [
    [0, 0],
    [80, 0],
    [80, 40],
    [0, 40],
    [0, 0],
  ]);
  assert.deepEqual(firstPrimitivePoints(document)[0], [10, 20]);
});

test('autoRepairSketchDocumentFromBrepProjection preserves closed duplicate point', () => {
  const result = autoRepairSketchDocumentFromBrepProjection(document, projection());
  const points = firstPrimitivePoints(result.document);

  assert.deepEqual(points[0], points.at(-1));
});

test('autoRepairSketchDocumentFromBrepProjection skips wrong primitive id', () => {
  const result = autoRepairSketchDocumentFromBrepProjection(document, projection('missing-primitive'));

  assert.equal(result.repaired, false);
  assert.deepEqual(result.evidence, []);
  assert.deepEqual(result.document, document);
});

test('autoRepairSketchDocumentFromBrepProjection infers primitive from single closed polyline when issue omits primitive id', () => {
  const result = autoRepairSketchDocumentFromBrepProjection(document, projection(null));

  assert.equal(result.repaired, true);
  assert.equal(result.evidence[0]?.primitiveId, 'primitive-front');
  assert.deepEqual(firstPrimitivePoints(result.document), [
    [0, 0],
    [80, 0],
    [80, 40],
    [0, 40],
    [0, 0],
  ]);
});

test('autoRepairSketchDocumentFromBrepProjection expands closed polyline to contain outside BRep projection bounds', () => {
  const result = autoRepairSketchDocumentFromBrepProjection(document, containmentProjection());

  assert.equal(result.repaired, true);
  assert.equal(result.evidence[0]?.primitiveId, 'primitive-front');
  assert.match(result.evidence[0]?.detail ?? '', /BREP AUTO CONTAIN FRONT primitive-front bounds 50x30 -> 54\.2x30/);
  assert.deepEqual(firstPrimitivePoints(result.document), [
    [10, 20],
    [64.2, 20],
    [64.2, 50],
    [10, 50],
    [10, 20],
  ]);
});

test('autoRepairSketchDocumentFromBrepProjection skips containment mismatch when projection bounds already fit source bounds', () => {
  const sameBounds = containmentProjection();
  sameBounds.views = [
    {
      view: 'front',
      direction: [0, -1, 0],
      visibleEdges: [
        { edgeId: 'front-bottom', points: [[10, 20], [60, 20]], sourceClass: 'V' },
        { edgeId: 'front-right', points: [[60, 20], [60, 50]], sourceClass: 'V' },
      ],
      hiddenEdges: [
        { edgeId: 'front-top', points: [[10, 50], [60, 50]], sourceClass: 'H' },
      ],
    },
  ];

  const result = autoRepairSketchDocumentFromBrepProjection(document, sameBounds);

  assert.equal(result.repaired, false);
  assert.deepEqual(result.evidence, []);
  assert.deepEqual(result.document, document);
});

test('autoRepairSketchDocumentFromBrepProjection skips huge containment expansion', () => {
  const result = autoRepairSketchDocumentFromBrepProjection(document, hugeContainmentProjection());

  assert.equal(result.repaired, false);
  assert.deepEqual(result.evidence, []);
  assert.deepEqual(result.document, document);
});

test('autoRepairSketchDocumentFromBrepProjection skips all repair when validation includes unknown issue', () => {
  const mixedProjection = containmentProjection();
  mixedProjection.validation?.issues?.push({
    sketchId: 'sketch-front',
    primitiveId: 'primitive-front',
    severity: 'error',
    message: 'raw BREP/SKETCH topology mismatch: face loop cannot be matched',
  });

  const result = autoRepairSketchDocumentFromBrepProjection(document, mixedProjection);

  assert.equal(result.repaired, false);
  assert.deepEqual(result.evidence, []);
  assert.deepEqual(result.document, document);
});

test('autoRepairSketchDocumentFromBrepProjection skips primitive inference when sketch has multiple candidates', () => {
  const ambiguousDocument: SketchDocument = {
    ...document,
    sketches: [
      {
        sketchId: 'sketch-front',
        view: 'front',
        primitives: [
          ...(document.sketches?.[0]?.primitives ?? []),
          {
            primitiveId: 'primitive-front-second',
            kind: 'polyline',
            points: [
              [1, 1],
              [3, 1],
              [3, 3],
              [1, 3],
              [1, 1],
            ],
            closed: true,
          },
        ],
      },
    ],
  };

  const result = autoRepairSketchDocumentFromBrepProjection(ambiguousDocument, projection(null));

  assert.equal(result.repaired, false);
  assert.deepEqual(result.evidence, []);
  assert.deepEqual(result.document, ambiguousDocument);
});

test('autoRepairSketchDocumentFromBrepProjection skips unsupported primitive kind', () => {
  const circleDocument: SketchDocument = {
    ...document,
    sketches: [
      {
        sketchId: 'sketch-front',
        view: 'front',
        primitives: [
          {
            primitiveId: 'primitive-front',
            kind: 'circle',
            points: [[35, 35]],
            radius: 10,
            closed: true,
          },
        ],
      },
    ],
  };

  const result = autoRepairSketchDocumentFromBrepProjection(circleDocument, projection());

  assert.equal(result.repaired, false);
  assert.deepEqual(result.evidence, []);
  assert.deepEqual(result.document, circleDocument);
});

test('autoRepairSketchDocumentFromBrepProjection skips passing validation', () => {
  const passingProjection = projection();
  passingProjection.validation = { passed: true, issues: [], evidence: ['ok'] };

  const result = autoRepairSketchDocumentFromBrepProjection(document, passingProjection);

  assert.equal(result.repaired, false);
  assert.deepEqual(result.evidence, []);
  assert.deepEqual(result.document, document);
});
