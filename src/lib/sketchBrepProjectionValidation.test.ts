import assert from 'node:assert/strict';
import test from 'node:test';

import {
  buildSketchBrepProjectionRepairTargets,
  buildSketchBrepProjectionValidationSummary,
  sketchBrepProjectionBoundsSeed,
} from './sketchBrepProjectionValidation';
import type { BrepHiddenLineProjectionResponse, SketchDocument } from './tauri/contracts';

const document: SketchDocument = {
  documentId: 'doc-1',
  units: 'mm',
  sketches: [
    {
      sketchId: 'front-sketch',
      view: 'front',
      primitives: [
        {
          primitiveId: 'front-profile',
          kind: 'polyline',
          closed: true,
          points: [
            [0, 0],
            [20, 0],
            [20, 10],
            [0, 10],
            [0, 0],
          ],
        },
      ],
    },
  ],
};

const matchingProjection: BrepHiddenLineProjectionResponse = {
  modelId: 'model-1',
  sourceArtifactPath: '/tmp/model.FCStd',
  views: [
    {
      view: 'front',
      direction: [0, -1, 0],
      visibleEdges: [
        {
          edgeId: 'front-visible-bottom',
          points: [
            [0, 0],
            [20, 0],
          ],
          sourceClass: 'V',
        },
        {
          edgeId: 'front-visible-right',
          points: [
            [20, 0],
            [20, 10],
          ],
          sourceClass: 'V',
        },
      ],
      hiddenEdges: [
        {
          edgeId: 'front-hidden-top',
          points: [
            [20, 10],
            [0, 10],
          ],
          sourceClass: 'H',
        },
        {
          edgeId: 'front-hidden-left',
          points: [
            [0, 10],
            [0, 0],
          ],
          sourceClass: 'H',
        },
      ],
    },
  ],
  warnings: [],
};

test('Given SketchDocument and matching BRep projection When summary builds Then rows pass with bounds seed', () => {
  const summary = buildSketchBrepProjectionValidationSummary(document, matchingProjection);

  assert.deepEqual(summary.rows, [
    {
      label: 'BRep projection',
      status: 'pass',
      evidence: 'model-1; model.FCStd; 1 view.',
    },
    {
      label: 'FRONT bounds',
      status: 'pass',
      evidence: 'sketch 20 x 10; projection 20 x 10; 2 visible / 2 hidden.',
    },
  ]);
  assert.deepEqual(summary.boundsComparisonSeed, {
    documentId: 'doc-1',
    units: 'mm',
    views: [
      {
        view: 'front',
        sketchBounds: { minX: 0, minY: 0, maxX: 20, maxY: 10, width: 20, height: 10 },
        projectionBounds: { minX: 0, minY: 0, maxX: 20, maxY: 10, width: 20, height: 10 },
        visibleEdgeCount: 2,
        hiddenEdgeCount: 2,
        edgeCount: 4,
      },
    ],
  });
  assert.deepEqual(summary.viewSummaries, [
    {
      view: 'front',
      visibleEdgeCount: 2,
      hiddenEdgeCount: 2,
      edgeCount: 4,
      boundsMatched: true,
    },
  ]);
});

test('Given projection extents differ from SketchDocument When summary builds Then bounds row fails', () => {
  const projection: BrepHiddenLineProjectionResponse = {
    ...matchingProjection,
    views: [
      {
        ...matchingProjection.views![0],
        visibleEdges: [
          {
            edgeId: 'front-visible-bottom',
            points: [
              [0, 0],
              [20, 0],
            ],
            sourceClass: 'V',
          },
          {
            edgeId: 'front-visible-short-right',
            points: [
              [20, 0],
              [20, 8],
            ],
            sourceClass: 'V',
          },
        ],
        hiddenEdges: [
          {
            edgeId: 'front-hidden-short-top',
            points: [
              [20, 8],
              [0, 8],
            ],
            sourceClass: 'H',
          },
        ],
      },
    ],
  };

  const summary = buildSketchBrepProjectionValidationSummary(document, projection);

  assert.equal(summary.rows[1]?.status, 'fail');
  assert.equal(summary.rows[1]?.issue, 'bounds mismatch');
  assert.equal(summary.rows[1]?.evidence, 'sketch 20 x 10; projection 20 x 8; 2 visible / 1 hidden.');
});

test('Given backend BRep validation issue When repair targets build Then target is keyed by sketch primitive and view', () => {
  const targets = buildSketchBrepProjectionRepairTargets(document, {
    ...matchingProjection,
    validation: {
      passed: false,
      issues: [
        {
          sketchId: 'front-sketch',
          primitiveId: 'front-profile',
          severity: 'error',
          message: 'raw BREP/SKETCH bounds mismatch: front sketch bounds x=0..20 y=0..10; OCCT bounds x=0..20 y=0..8',
        },
      ],
      evidence: ['backend BRep/sketch validation failed'],
    },
  });

  assert.deepEqual(targets, [
    {
      targetId: 'brep-repair-front-sketch-front-profile-0',
      sketchId: 'front-sketch',
      primitiveId: 'front-profile',
      view: 'front',
      severity: 'error',
      label: 'FRONT / front-profile',
      reason: 'raw BREP/SKETCH bounds mismatch: front sketch bounds x=0..20 y=0..10; OCCT bounds x=0..20 y=0..8',
      evidence:
        'front-sketch / front-profile / raw BREP/SKETCH bounds mismatch: front sketch bounds x=0..20 y=0..10; OCCT bounds x=0..20 y=0..8',
    },
  ]);
});

test('Given passing backend BRep validation When repair targets build Then no targets are emitted', () => {
  const targets = buildSketchBrepProjectionRepairTargets(document, {
    ...matchingProjection,
    validation: {
      passed: true,
      issues: [],
      evidence: ['backend BRep/sketch validation passed'],
    },
  });

  assert.deepEqual(targets, []);
});

test('Given missing BRep projection When summary builds Then projection row pending', () => {
  const summary = buildSketchBrepProjectionValidationSummary(document, null);

  assert.deepEqual(summary.rows, [
    {
      label: 'BRep projection',
      status: 'pending',
      evidence: 'Waiting for hidden-line projection evidence.',
      issue: 'missing brep projection',
    },
  ]);
  assert.deepEqual(summary.boundsComparisonSeed.views, []);
});

test('Given BRep projection has no edge points for sketch view When summary builds Then view bounds row fails', () => {
  const summary = buildSketchBrepProjectionValidationSummary(document, {
    ...matchingProjection,
    views: [
      {
        view: 'front',
        direction: [0, -1, 0],
        visibleEdges: [],
        hiddenEdges: [],
      },
    ],
  });

  assert.deepEqual(summary.rows, [
    {
      label: 'BRep projection',
      status: 'pass',
      evidence: 'model-1; model.FCStd; 1 view.',
    },
    {
      label: 'FRONT bounds',
      status: 'fail',
      evidence: 'front sketch has no matching BRep projection edges.',
      issue: 'missing brep projection bounds',
    },
  ]);
});

test('Given hidden edges extend projection bounds When seed builds Then hidden edges count and affect bounds', () => {
  const projection: BrepHiddenLineProjectionResponse = {
    ...matchingProjection,
    views: [
      {
        ...matchingProjection.views![0],
        visibleEdges: [
          {
            edgeId: 'front-visible-small',
            points: [
              [0, 0],
              [10, 0],
            ],
            sourceClass: 'V',
          },
        ],
        hiddenEdges: [
          {
            edgeId: 'front-hidden-full',
            points: [
              [20, 10],
              [0, 10],
            ],
            sourceClass: 'H',
          },
        ],
      },
    ],
  };

  const seed = sketchBrepProjectionBoundsSeed(document, projection);

  assert.deepEqual(seed.views[0], {
    view: 'front',
    sketchBounds: { minX: 0, minY: 0, maxX: 20, maxY: 10, width: 20, height: 10 },
    projectionBounds: { minX: 0, minY: 0, maxX: 20, maxY: 10, width: 20, height: 10 },
    visibleEdgeCount: 1,
    hiddenEdgeCount: 1,
    edgeCount: 2,
  });
});
