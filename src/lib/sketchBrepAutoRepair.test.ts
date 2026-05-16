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
    validation: {
      passed: false,
      issues: [
        {
          sketchId: 'sketch-front',
          view: 'front',
          primitiveId: issuePrimitiveId,
          kind: 'boundsMismatch',
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
          view: 'front',
          primitiveId: 'primitive-front',
          kind: 'containmentMismatch',
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

test('autoRepairSketchDocumentFromBrepProjection uses structured bounds kind when message is neutral', () => {
  const neutral = projection();
  if (!neutral.validation?.issues?.[0]) assert.fail('bounds issue missing');
  neutral.validation.issues[0].kind = 'boundsMismatch';
  neutral.validation.issues[0].message = 'projection envelope deviates from source profile';

  const result = autoRepairSketchDocumentFromBrepProjection(document, neutral);

  assert.equal(result.repaired, true);
  assert.equal(result.evidence[0]?.primitiveId, 'primitive-front');
  assert.match(result.evidence[0]?.detail ?? '', /BREP AUTO SNAP FRONT primitive-front/);
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

test('autoRepairSketchDocumentFromBrepProjection skips bounds repair when issue omits primitive id and topology', () => {
  const result = autoRepairSketchDocumentFromBrepProjection(document, projection(null));

  assert.equal(result.repaired, false);
  assert.deepEqual(result.evidence, []);
  assert.deepEqual(result.document, document);
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

test('autoRepairSketchDocumentFromBrepProjection uses structured containment kind when message is neutral', () => {
  const neutral = containmentProjection();
  if (!neutral.validation?.issues?.[0]) assert.fail('containment issue missing');
  neutral.validation.issues[0].kind = 'containmentMismatch';
  neutral.validation.issues[0].message = 'projection exits source profile, maxOutside=4.2mm';

  const result = autoRepairSketchDocumentFromBrepProjection(document, neutral);

  assert.equal(result.repaired, true);
  assert.equal(result.evidence[0]?.primitiveId, 'primitive-front');
  assert.match(result.evidence[0]?.detail ?? '', /BREP AUTO CONTAIN FRONT primitive-front/);
});

test('autoRepairSketchDocumentFromBrepProjection prefers explicit edge locator over stale primitive id', () => {
  const multiLoopDocument: SketchDocument = {
    documentId: 'doc-brep-repair-multiloop',
    activeSketchId: 'sketch-front',
    units: 'mm',
    sketches: [
      {
        sketchId: 'sketch-front',
        view: 'front',
        primitives: [
          {
            primitiveId: 'primitive-front-outer',
            kind: 'polyline',
            closed: true,
            points: [
              [10, 20],
              [60, 20],
              [60, 50],
              [10, 50],
              [10, 20],
            ],
            topology: {
              loopId: 'front-outer',
              edgeIds: ['outer-bottom', 'outer-right', 'outer-top', 'outer-left'],
              loopRole: 'outer',
              sourceClass: 'derived',
            },
          },
          {
            primitiveId: 'primitive-front-hole',
            kind: 'polyline',
            closed: true,
            points: [
              [25, 28],
              [35, 28],
              [35, 38],
              [25, 38],
              [25, 28],
            ],
            topology: {
              loopId: 'front-hole',
              edgeIds: ['inner-bottom', 'inner-right', 'inner-top', 'inner-left'],
              loopRole: 'hole',
              sourceClass: 'derived',
            },
          },
        ],
      },
    ],
  };

  const result = autoRepairSketchDocumentFromBrepProjection(multiLoopDocument, {
    modelId: 'model-1',
    sourceArtifactPath: '/tmp/model.step',
    views: [
      {
        view: 'front',
        direction: [0, -1, 0],
        visibleEdges: [
          { edgeId: 'outer-bottom', points: [[0, 0], [80, 0]], sourceClass: 'V' },
          { edgeId: 'outer-right', points: [[80, 0], [80, 40]], sourceClass: 'V' },
        ],
        hiddenEdges: [
          { edgeId: 'outer-top', points: [[0, 40], [80, 40]], sourceClass: 'H' },
        ],
      },
    ],
    validation: {
      passed: false,
      issues: [
        {
          sketchId: 'sketch-front',
          view: 'front',
          primitiveId: 'primitive-front-hole',
          edgeId: 'outer-right',
          kind: 'boundsMismatch',
          severity: 'error',
          message: 'raw BREP/SKETCH bounds mismatch: projection bounds exceed source profile',
        },
      ],
      evidence: [],
    },
  });

  assert.equal(result.repaired, true);
  assert.equal(result.evidence[0]?.primitiveId, 'primitive-front-outer');
  assert.deepEqual(result.document.sketches?.[0]?.primitives?.[0]?.points, [
    [0, 0],
    [80, 0],
    [80, 40],
    [0, 40],
    [0, 0],
  ]);
  assert.deepEqual(result.document.sketches?.[0]?.primitives?.[1]?.points, [
    [25, 28],
    [35, 28],
    [35, 38],
    [25, 38],
    [25, 28],
  ]);
});

test('autoRepairSketchDocumentFromBrepProjection prefers topology locator over stale primitive id for containment', () => {
  const multiLoopDocument: SketchDocument = {
    documentId: 'doc-brep-repair-multiloop-topology',
    activeSketchId: 'sketch-front',
    units: 'mm',
    sketches: [
      {
        sketchId: 'sketch-front',
        view: 'front',
        primitives: [
          {
            primitiveId: 'primitive-front-outer',
            kind: 'polyline',
            closed: true,
            points: [
              [0, 0],
              [80, 0],
              [80, 50],
              [0, 50],
              [0, 0],
            ],
            topology: {
              loopId: 'front-outer',
              edgeIds: ['outer-a', 'outer-b', 'outer-c', 'outer-d'],
              loopRole: 'outer',
              sourceClass: 'derived',
            },
          },
          {
            primitiveId: 'primitive-front-hole',
            kind: 'polyline',
            closed: true,
            points: [
              [25, 18],
              [45, 18],
              [45, 34],
              [25, 34],
              [25, 18],
            ],
            topology: {
              loopId: 'front-hole',
              edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
              loopRole: 'hole',
              sourceClass: 'derived',
            },
          },
        ],
      },
    ],
  };

  const result = autoRepairSketchDocumentFromBrepProjection(multiLoopDocument, {
    modelId: 'model-1',
    sourceArtifactPath: '/tmp/model.step',
    views: [
      {
        view: 'front',
        direction: [0, -1, 0],
        visibleEdges: [
          { edgeId: 'front-v0', points: [[0, 0], [80, 0]], sourceClass: 'V' },
          { edgeId: 'front-v1', points: [[46, 18], [46, 35]], sourceClass: 'V1' },
        ],
        hiddenEdges: [{ edgeId: 'front-h0', points: [[0, 50], [80, 50]], sourceClass: 'H' }],
        loops: [
          {
            loopId: 'front-outer',
            edgeIds: ['outer-a', 'outer-b', 'outer-c', 'outer-d'],
            points: [[0, 0], [80, 0], [80, 50], [0, 50], [0, 0]],
            role: 'outer',
            sourceClass: 'derived',
          },
          {
            loopId: 'front-hole',
            edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
            points: [[25, 18], [46, 18], [46, 35], [25, 35], [25, 18]],
            role: 'hole',
            sourceClass: 'derived',
          },
        ],
      },
    ],
    validation: {
      passed: false,
      issues: [
        {
          sketchId: 'sketch-front',
          view: 'front',
          primitiveId: 'primitive-front-outer',
          kind: 'containmentMismatch',
          severity: 'error',
          message: 'projection exits source profile, maxOutside=4.2mm',
          topology: {
            loopId: 'front-hole',
            edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
            loopRole: 'hole',
            sourceClass: 'derived',
          },
        },
      ],
      evidence: [],
    },
  });

  assert.equal(result.repaired, true);
  assert.equal(result.evidence[0]?.primitiveId, 'primitive-front-hole');
  assert.match(result.evidence[0]?.detail ?? '', /BREP AUTO CONTAIN FRONT primitive-front-hole bounds 20x16 -> 21x17/);
  assert.deepEqual(result.document.sketches?.[0]?.primitives?.[0]?.points, [
    [0, 0],
    [80, 0],
    [80, 50],
    [0, 50],
    [0, 0],
  ]);
  assert.deepEqual(result.document.sketches?.[0]?.primitives?.[1]?.points, [
    [25, 18],
    [46, 18],
    [46, 35],
    [25, 35],
    [25, 18],
  ]);
});

test('autoRepairSketchDocumentFromBrepProjection uses targeted hole bounds for bounds mismatch', () => {
  const multiLoopDocument: SketchDocument = {
    documentId: 'doc-brep-repair-multiloop-bounds',
    activeSketchId: 'sketch-front',
    units: 'mm',
    sketches: [
      {
        sketchId: 'sketch-alpha',
        view: 'front',
        primitives: [
          {
            primitiveId: 'primitive-front-outer',
            kind: 'polyline',
            closed: true,
            points: [
              [0, 0],
              [80, 0],
              [80, 50],
              [0, 50],
              [0, 0],
            ],
            topology: {
              loopId: 'front-outer',
              edgeIds: ['outer-a', 'outer-b', 'outer-c', 'outer-d'],
              loopRole: 'outer',
              sourceClass: 'derived',
            },
          },
          {
            primitiveId: 'primitive-front-hole',
            kind: 'polyline',
            closed: true,
            points: [
              [25, 18],
              [45, 18],
              [45, 34],
              [25, 34],
              [25, 18],
            ],
            topology: {
              loopId: 'front-hole',
              edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
              loopRole: 'hole',
              sourceClass: 'derived',
            },
          },
        ],
      },
    ],
  };

  const result = autoRepairSketchDocumentFromBrepProjection(multiLoopDocument, {
    modelId: 'model-1',
    sourceArtifactPath: '/tmp/model.step',
    views: [
      {
        view: 'front',
        direction: [0, -1, 0],
        visibleEdges: [
          { edgeId: 'outer-a', points: [[0, 0], [80, 0]], sourceClass: 'V' },
          { edgeId: 'outer-b', points: [[80, 0], [80, 50]], sourceClass: 'V' },
          { edgeId: 'outer-c', points: [[80, 50], [0, 50]], sourceClass: 'V' },
          { edgeId: 'outer-d', points: [[0, 50], [0, 0]], sourceClass: 'V' },
          { edgeId: 'inner-a', points: [[25, 18], [46, 18]], sourceClass: 'V' },
          { edgeId: 'inner-b', points: [[46, 18], [46, 35]], sourceClass: 'V' },
          { edgeId: 'inner-c', points: [[46, 35], [25, 35]], sourceClass: 'V' },
          { edgeId: 'inner-d', points: [[25, 35], [25, 18]], sourceClass: 'V' },
        ],
        hiddenEdges: [],
        loops: [
          {
            loopId: 'front-outer',
            edgeIds: ['outer-a', 'outer-b', 'outer-c', 'outer-d'],
            points: [[0, 0], [80, 0], [80, 50], [0, 50], [0, 0]],
            role: 'outer',
            sourceClass: 'derived',
          },
          {
            loopId: 'front-hole',
            edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
            points: [[25, 18], [46, 18], [46, 35], [25, 35], [25, 18]],
            role: 'hole',
            sourceClass: 'derived',
          },
        ],
      },
    ],
    validation: {
      passed: false,
      issues: [
        {
          sketchId: 'sketch-alpha',
          view: 'front',
          primitiveId: 'primitive-front-outer',
          kind: 'boundsMismatch',
          severity: 'error',
          message: 'projection envelope deviates from source profile',
          topology: {
            loopId: 'front-hole',
            edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
            loopRole: 'hole',
            sourceClass: 'derived',
          },
        },
      ],
      evidence: [],
    },
  });

  assert.equal(result.repaired, true);
  assert.equal(result.evidence[0]?.primitiveId, 'primitive-front-hole');
  assert.match(result.evidence[0]?.detail ?? '', /BREP AUTO SNAP FRONT primitive-front-hole bounds 20x16 -> 21x17/);
  assert.deepEqual(result.document.sketches?.[0]?.primitives?.[0]?.points, [
    [0, 0],
    [80, 0],
    [80, 50],
    [0, 50],
    [0, 0],
  ]);
  assert.deepEqual(result.document.sketches?.[0]?.primitives?.[1]?.points, [
    [25, 18],
    [46, 18],
    [46, 35],
    [25, 35],
    [25, 18],
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

test('autoRepairSketchDocumentFromBrepProjection repairs supported issues even when validation also includes topology issue', () => {
  const mixedProjection = containmentProjection();
  mixedProjection.validation?.issues?.push({
    sketchId: 'sketch-front',
    view: 'front',
    primitiveId: 'primitive-front',
    kind: 'topologyMismatch',
    severity: 'error',
    message: 'raw BREP/SKETCH topology mismatch: face loop cannot be matched',
  });

  const result = autoRepairSketchDocumentFromBrepProjection(document, mixedProjection);

  assert.equal(result.repaired, true);
  assert.match(result.evidence[0]?.detail ?? '', /BREP AUTO CONTAIN FRONT/i);
  assert.notDeepEqual(result.document, document);
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
